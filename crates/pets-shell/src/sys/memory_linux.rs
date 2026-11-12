//! Память процесса на Linux для бенча: из `/proc/self/smaps_rollup` — своя резидентная (`Private_Clean` +
//! `Private_Dirty`) и выгруженная (`SwapPss`) память без общих страниц; ближайшее к `phys_footprint` macOS.

/// Сумма полей разбивки, байты; полей нет — `None`.
fn footprint_of(rollup: &str) -> Option<u64> {
    let mut sum = None;
    for line in rollup.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        if !matches!(key, "Private_Clean" | "Private_Dirty" | "SwapPss") {
            continue;
        }
        let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
        sum = Some(sum.unwrap_or(0) + kb * 1024);
    }
    sum
}

/// Своя память этого процесса, байты; ядро без `smaps_rollup` (до 4.14) — `None`.
pub fn phys_footprint() -> Option<u64> {
    footprint_of(&std::fs::read_to_string("/proc/self/smaps_rollup").ok()?)
}

#[cfg(test)]
mod tests {
    /// Разбор полей и живой процесс.
    #[test]
    fn footprint_is_reported() {
        let rollup = "Rss: 100 kB\nPrivate_Clean: 1 kB\nPrivate_Dirty: 2 kB\nSwapPss: 3 kB\n";
        assert_eq!(super::footprint_of(rollup), Some(6 * 1024));
        assert!(super::phys_footprint().is_some_and(|b| b > 0));
    }
}
