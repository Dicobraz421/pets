//! Память процесса на macOS для бенча: `phys_footprint` — столбец «Память» Мониторинга системы (то же число, что
//! `task_info(TASK_VM_INFO).phys_footprint`): своя резидентная и сжатая память без общих страниц.

/// `phys_footprint` этого процесса, байты; ОС не ответила — `None`.
pub fn phys_footprint() -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: буфер — `rusage_info_v2` под флейвор `RUSAGE_INFO_V2`; свой pid.
    let rc = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as libc::c_int,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    };
    // SAFETY: при успехе ядро заполнило структуру; до этого она обнулена.
    (rc == 0).then(|| unsafe { info.assume_init() }.ri_phys_footprint)
}

#[cfg(test)]
mod tests {
    /// Процесс что-то занимает.
    #[test]
    fn footprint_is_reported() {
        assert!(super::phys_footprint().is_some_and(|b| b > 0));
    }
}
