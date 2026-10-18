use anyhow::{Context, Result, ensure};
use std::{
    collections::VecDeque,
    fs::File,
    io::Read,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::{
            BOOL, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation, WAIT_OBJECT_0,
        },
        Security::SECURITY_ATTRIBUTES,
        System::{
            JobObjects::{
                CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
            },
            Pipes::CreatePipe,
            Threading::{
                CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
                DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
                InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
                UpdateProcThreadAttribute, WaitForSingleObject,
            },
        },
    },
    core::{HSTRING, PCWSTR, PWSTR},
};

pub const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Default)]
struct Output {
    bytes: VecDeque<u8>,
    dropped: u64,
    finished: bool,
}

impl Output {
    fn push(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.bytes.len() == OUTPUT_LIMIT {
                self.bytes.pop_front();
                self.dropped = self.dropped.saturating_add(1);
            }
            self.bytes.push_back(*byte);
        }
    }
}

fn handle(value: &OwnedHandle) -> HANDLE {
    HANDLE(value.as_raw_handle())
}

/// Creation-returned handles are owned exactly once. Never reopen by PID/name.
unsafe fn own(value: HANDLE) -> OwnedHandle {
    unsafe { OwnedHandle::from_raw_handle(value.0) }
}

fn pipe() -> Result<(OwnedHandle, OwnedHandle)> {
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        bInheritHandle: BOOL(1),
        ..Default::default()
    };
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    // SAFETY: Both returned pipe handles have unique ownership and valid buffers.
    unsafe {
        CreatePipe(&mut read, &mut write, Some(&security), 0)?;
        Ok((own(read), own(write)))
    }
}

struct Attributes {
    _storage: Vec<u128>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl Attributes {
    fn new() -> Result<Self> {
        let mut size = 0;
        // SAFETY: First call obtains the required size. u128 storage provides
        // native allocation alignment and stays fixed until the list is deleted.
        unsafe {
            let _ = InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST::default(),
                2,
                0,
                &mut size,
            );
            ensure!(size > 0, "Cannot size the service launch attributes.");
            let mut storage = vec![0u128; size.div_ceil(std::mem::size_of::<u128>())];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
            InitializeProcThreadAttributeList(list, 2, 0, &mut size)?;
            Ok(Self {
                _storage: storage,
                list,
            })
        }
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: Successfully initialized list; storage remains alive here.
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

pub struct ManagedProcess {
    job: OwnedHandle,
    process: OwnedHandle,
    pub id: u32,
    output: Arc<Mutex<Output>>,
}

impl ManagedProcess {
    pub fn start(executable: &Path, arguments: &str, folder: &Path) -> Result<Self> {
        Self::start_environment(executable, arguments, folder, None)
    }

    pub fn start_environment(
        executable: &Path,
        arguments: &str,
        folder: &Path,
        environment: Option<&[u16]>,
    ) -> Result<Self> {
        ensure!(
            executable.is_file(),
            "Executable does not exist: {}",
            executable.display()
        );
        ensure!(
            folder.is_absolute() && folder.is_dir(),
            "Working folder does not exist: {}",
            folder.display()
        );
        let native_path: PathBuf = executable.components().collect();
        let executable = HSTRING::from(native_path.as_os_str());
        let folder = HSTRING::from(folder.as_os_str());
        let mut command: Vec<u16> = std::iter::once(b'"' as u16)
            .chain(executable.as_wide().iter().copied())
            .chain([b'"' as u16, b' ' as u16])
            .chain(arguments.encode_utf16())
            .chain(Some(0))
            .collect();
        let (read, write) = pipe()?;
        let (input, input_write) = pipe()?;
        drop(input_write); // Noninteractive workloads receive EOF on stdin.
        // SAFETY: This is our output pipe's parent-only end.
        unsafe {
            SetHandleInformation(handle(&read), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))?;
        }
        let output = Arc::new(Mutex::new(Output::default()));
        let reader_output = output.clone();
        let mut reader = File::from(read);
        std::thread::Builder::new()
            .name("service-output".into())
            .spawn(move || {
                let mut buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => reader_output.lock().unwrap().push(&buffer[..count]),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                reader_output.lock().unwrap().finished = true;
            })
            .context("Cannot start service output capture.")?;
        // SAFETY: The new job has no kill-on-close or breakaway limits. Job-list
        // assignment happens during creation, before user code can spawn children.
        // Only the two explicitly listed pipe handles can be inherited.
        unsafe {
            let job = own(CreateJobObjectW(None, PCWSTR::null())?);
            let inherited = [handle(&input), handle(&write)];
            let jobs = [handle(&job)];
            let attributes = Attributes::new()?;
            for (kind, values) in [
                (PROC_THREAD_ATTRIBUTE_HANDLE_LIST, inherited.as_slice()),
                (PROC_THREAD_ATTRIBUTE_JOB_LIST, jobs.as_slice()),
            ] {
                UpdateProcThreadAttribute(
                    attributes.list,
                    0,
                    kind as usize,
                    Some(values.as_ptr().cast()),
                    std::mem::size_of_val(values),
                    None,
                    None,
                )?;
            }
            let mut startup = STARTUPINFOEXW {
                lpAttributeList: attributes.list,
                ..Default::default()
            };
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = handle(&input);
            startup.StartupInfo.hStdOutput = handle(&write);
            startup.StartupInfo.hStdError = handle(&write);
            let mut process = PROCESS_INFORMATION::default();
            CreateProcessW(
                &executable,
                PWSTR(command.as_mut_ptr()),
                None,
                None,
                BOOL(1),
                CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.map(|value| value.as_ptr().cast()),
                &folder,
                &startup.StartupInfo,
                &mut process,
            )
            .context("Cannot start the command in its owned Windows job.")?;
            let root = own(process.hProcess);
            drop(own(process.hThread));
            Ok(Self {
                job,
                process: root,
                id: process.dwProcessId,
                output,
            })
        }
    }

    pub fn active(&self) -> Result<bool> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: Query only this creation-owned job, with a correctly sized buffer.
        unsafe {
            QueryInformationJobObject(
                handle(&self.job),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                None,
            )?;
        }
        Ok(info.ActiveProcesses != 0)
    }

    pub fn exit_code(&self) -> Result<Option<u32>> {
        // SAFETY: A retained process handle identifies the original incarnation.
        unsafe {
            if WaitForSingleObject(handle(&self.process), 0) != WAIT_OBJECT_0 {
                return Ok(None);
            }
            let mut code = 0;
            GetExitCodeProcess(handle(&self.process), &mut code)?;
            Ok(Some(code))
        }
    }

    pub fn output_pending(&self) -> bool {
        !self.output.lock().unwrap().finished
    }

    pub fn output(&self) -> String {
        let output = self.output.lock().unwrap();
        let bytes: Vec<_> = output.bytes.iter().copied().collect();
        let text = String::from_utf8_lossy(&bytes);
        if output.dropped == 0 {
            text.into_owned()
        } else {
            format!("[Earlier output omitted: {} bytes]\n{text}", output.dropped)
        }
    }

    /// Force an owned command job. Workload termination requires explicit Force
    /// stop; internal read-only Docker log clients can also be cancelled on cleanup.
    pub fn force_stop(&self) -> Result<()> {
        // SAFETY: This handle can refer only to the job created for this workload.
        unsafe {
            TerminateJobObject(handle(&self.job), 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_tail_stays_bounded_across_large_chunks_without_newlines() {
        let mut output = Output::default();
        output.push(&vec![b'a'; OUTPUT_LIMIT + 100]);
        output.push(b"latest");
        assert_eq!(output.bytes.len(), OUTPUT_LIMIT);
        assert_eq!(output.dropped, 106);
        assert!(
            output
                .bytes
                .iter()
                .rev()
                .take(6)
                .copied()
                .eq(b"latest".iter().rev().copied())
        );
    }
}
