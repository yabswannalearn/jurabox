use anyhow::Result;
use windows::Win32::{
    Foundation::{CloseHandle, FILETIME},
    System::{
        ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        },
        Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
};

#[derive(Clone, serde::Serialize)]
pub struct Sample {
    pub cpu_seconds: f64,
    pub working_set_bytes: usize,
    pub private_bytes: usize,
}
pub fn sample(pid: u32) -> Result<Sample> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)?;
        let result = (|| -> Result<Sample> {
            let mut created = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user)?;
            let mut memory = PROCESS_MEMORY_COUNTERS_EX {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                ..Default::default()
            };
            GetProcessMemoryInfo(
                process,
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
                memory.cb,
            )?;
            let time = |value: FILETIME| {
                ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
            };
            Ok(Sample {
                cpu_seconds: (time(kernel) + time(user)) as f64 / 10_000_000.,
                working_set_bytes: memory.WorkingSetSize,
                private_bytes: memory.PrivateUsage,
            })
        })();
        let _ = CloseHandle(process);
        result
    }
}
