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
            BOOL, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE,
            HANDLE_FLAG_INHERIT, HANDLE_FLAGS, HLOCAL, LocalFree, SetHandleInformation,
            WAIT_OBJECT_0,
        },
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        System::{
            JobObjects::{
                CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JobObjectBasicAccountingInformation, OpenJobObjectW, QueryInformationJobObject,
                TerminateJobObject,
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

/// Each returned kernel handle has exactly one Rust owner.
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
    process: Option<OwnedHandle>,
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
        Self::launch(executable, arguments, folder, environment, None)
    }

    /// The caller must flush this random capability to its recovery record first.
    pub fn start_recorded(
        executable: &Path,
        arguments: &str,
        folder: &Path,
        claim: uuid::Uuid,
        completion: &Path,
    ) -> Result<Self> {
        Self::launch(
            executable,
            arguments,
            folder,
            None,
            Some((claim, completion)),
        )
    }

    fn launch(
        executable: &Path,
        arguments: &str,
        folder: &Path,
        environment: Option<&[u16]>,
        claim: Option<(uuid::Uuid, &Path)>,
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
            let job = if let Some((claim, _)) = claim {
                create_recorded_job(claim)?
            } else {
                own(CreateJobObjectW(None, PCWSTR::null())?)
            };
            // Pin the job's name before any workload exists. Arm monitoring only
            // after this creation attempt finishes; EOF also handles owner death.
            let _registration = claim
                .map(|(claim, path)| crate::command_keeper::register(claim, path))
                .transpose()?;
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
                process: Some(root),
                id: process.dwProcessId,
                output,
            })
        }
    }

    pub fn active(&self) -> Result<bool> {
        job_active(&self.job)
    }

    pub fn exit_code(&self) -> Result<Option<u32>> {
        let Some(process) = &self.process else {
            return Ok(None);
        };
        // SAFETY: A retained process handle identifies the original incarnation.
        unsafe {
            if WaitForSingleObject(handle(process), 0) != WAIT_OBJECT_0 {
                return Ok(None);
            }
            let mut code = 0;
            GetExitCodeProcess(handle(process), &mut code)?;
            Ok(Some(code))
        }
    }

    pub fn recovered(&self) -> bool {
        self.process.is_none()
    }

    /// Reopen only the capability saved before creation, never a PID or executable.
    pub fn recover(claim: uuid::Uuid, completion: &Path) -> Result<Option<Self>> {
        ensure!(!claim.is_nil(), "Invalid command ownership capability.");
        let finished = crate::command_keeper::read_seal(completion, claim)?;
        let Some(job) = open_recorded_job(claim, 0x0004 | 0x0008)? else {
            ensure!(
                finished,
                "The original command job and its keeper are unavailable without a completion seal. Ownership is uncertain; replacement is blocked."
            );
            return Ok(None);
        };
        let process = Self {
            job,
            process: None,
            id: 0,
            output: Arc::new(Mutex::new(Output {
                finished: true,
                ..Default::default()
            })),
        };
        let active = process.active()?;
        ensure!(
            !active || !finished,
            "The command completion record conflicts with a live job; replacement is blocked."
        );
        if !active {
            crate::command_keeper::finish(completion, claim)?;
        }
        Ok(active.then_some(process))
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
        // SAFETY: Creation or the recorded private capability identifies this job.
        unsafe {
            TerminateJobObject(handle(&self.job), 1)?;
        }
        Ok(())
    }
}

fn job_name(claim: uuid::Uuid) -> HSTRING {
    HSTRING::from(format!(r"Global\Jurabox.Command.{claim}"))
}

pub(crate) fn open_recorded_job(claim: uuid::Uuid, access: u32) -> Result<Option<OwnedHandle>> {
    ensure!(!claim.is_nil(), "Invalid command ownership capability.");
    // SAFETY: Only original private capabilities are used. The keeper requests
    // query rights; service control additionally requests terminate rights.
    match unsafe { OpenJobObjectW(access, false, &job_name(claim)) } {
        Ok(job) => Ok(Some(unsafe { own(job) })),
        Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(None),
        Err(error) => {
            Err(error).context("Cannot verify the original command job; replacement is blocked.")
        }
    }
}

pub(crate) fn job_active(job: &OwnedHandle) -> Result<bool> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: A live original job handle and a correctly sized accounting buffer.
    unsafe {
        QueryInformationJobObject(
            handle(job),
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            std::mem::size_of_val(&info) as u32,
            None,
        )?;
    }
    Ok(info.ActiveProcesses != 0)
}

fn create_recorded_job(claim: uuid::Uuid) -> Result<OwnedHandle> {
    ensure!(!claim.is_nil(), "Invalid command ownership capability.");
    // SAFETY: Windows copies the owner/SYSTEM-only descriptor. It is freed on
    // both success and failure. A pre-existing object is never adopted on launch.
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from("D:P(A;;GA;;;SY)(A;;GA;;;OW)"),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let result = CreateJobObjectW(Some(&security), &job_name(claim));
        let error = GetLastError();
        let _ = LocalFree(HLOCAL(descriptor.0));
        let job = own(result?);
        ensure!(
            error != ERROR_ALREADY_EXISTS,
            "This command job already exists; no duplicate was started."
        );
        Ok(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_recorded_job_cannot_be_adopted_by_a_new_launch() {
        let claim = uuid::Uuid::new_v4();
        let _original = create_recorded_job(claim).unwrap();
        assert!(create_recorded_job(claim).is_err());
        assert!(
            open_recorded_job(uuid::Uuid::new_v4(), 0x0004)
                .unwrap()
                .is_none()
        );
        assert!(open_recorded_job(uuid::Uuid::nil(), 0x0004).is_err());
    }

    #[test]
    fn missing_job_without_completion_evidence_blocks_replacement() {
        let folder = tempfile::tempdir().unwrap();
        let claim = uuid::Uuid::new_v4();
        let path = folder.path().join("claim.json");
        crate::recovery::Record::new(path.clone())
            .save(&serde_json::json!({
                "version": 1, "claim": claim, "finished": false
            }))
            .unwrap();
        assert!(ManagedProcess::recover(claim, &path).is_err());
        crate::command_keeper::finish(&path, claim).unwrap();
        assert!(ManagedProcess::recover(claim, &path).unwrap().is_none());
    }

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
