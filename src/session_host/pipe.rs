use super::identity;
use anyhow::{Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
};
use windows::{
    Win32::{
        Foundation::{ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, LocalFree},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{
            FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX,
            SECURITY_ANONYMOUS, SECURITY_SQOS_PRESENT,
        },
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_BYTE, PIPE_WAIT,
        },
    },
    core::HSTRING,
};

// Includes all bounded service tails and project setups. Oversize input is rejected
// before allocation; a response too large fails closed without replaying an operation.
const FRAME_LIMIT: usize = 16 * 1024 * 1024;

pub(super) fn create(name: &str) -> Result<File> {
    // SAFETY: The descriptor belongs to this call, is copied into the pipe by
    // Windows, then freed. Only its owner and SYSTEM have access; remote clients
    // are rejected. There is exactly one instance, with first-instance protection.
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
        let handle = CreateNamedPipeW(
            &HSTRING::from(name),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65536,
            65536,
            0,
            Some(&security),
        );
        let error = std::io::Error::last_os_error();
        let _ = LocalFree(HLOCAL(descriptor.0));
        ensure!(
            !handle.is_invalid(),
            "Cannot create the private session pipe: {}",
            error
        );
        Ok(File::from_raw_handle(handle.0))
    }
}

pub(super) fn accept(pipe: &File, own: &identity::Identity) -> Result<u32> {
    // SAFETY: This file owns the server pipe for the complete connection lifetime.
    unsafe {
        if let Err(error) = ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None)
            && error.code() != ERROR_PIPE_CONNECTED.to_hresult()
        {
            return Err(error.into());
        }
        let mut pid = 0;
        GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &mut pid)?;
        let client = identity::inspect(pid)?;
        ensure!(
            client.executable == own.executable && client.session == own.session,
            "The session client belongs to another installation or Windows session."
        );
        Ok(pid)
    }
}

pub(super) fn connect(name: &str, expected: &identity::Identity) -> Result<File> {
    let pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(SECURITY_SQOS_PRESENT.0 | SECURITY_ANONYMOUS.0)
        .open(name)?;
    // SAFETY: Query the kernel-reported peer before sending credentials or commands.
    unsafe {
        let mut pid = 0;
        GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid)?;
        ensure!(
            identity::inspect(pid)? == *expected,
            "Session host identity changed."
        );
    }
    Ok(pipe)
}

pub(super) fn drain(file: &File) {
    // SAFETY: This live server pipe is about to disconnect. Windows waits until
    // its buffered response has been read, or reports that the client went away.
    // std::io::Write::flush for File does not provide this named-pipe guarantee.
    let _ = unsafe { FlushFileBuffers(HANDLE(file.as_raw_handle())) };
}

pub(super) fn write<T: Serialize>(file: &mut impl Write, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= FRAME_LIMIT,
        "Session message exceeds the size limit."
    );
    file.write_all(&(bytes.len() as u32).to_le_bytes())?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

pub(super) fn read<T: DeserializeOwned>(file: &mut impl Read) -> Result<T> {
    let mut length = [0; 4];
    file.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(
        length <= FRAME_LIMIT,
        "Session message exceeds the size limit."
    );
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_rejects_oversize_and_truncated_messages() {
        assert!(read::<String>(&mut &u32::MAX.to_le_bytes()[..]).is_err());
        let mut data = Vec::new();
        write(&mut data, &"Unicode 测试\nwith newlines").unwrap();
        assert_eq!(
            read::<String>(&mut &data[..]).unwrap(),
            "Unicode 测试\nwith newlines"
        );
        data.pop();
        assert!(read::<String>(&mut &data[..]).is_err());
    }
}
