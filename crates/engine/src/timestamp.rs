/// Format milliseconds as `HH:MM:SS.mmm`.
pub fn format_ms(ms: u64) -> String {
    let (h, rem) = (ms / 3_600_000, ms % 3_600_000);
    let (m, rem) = (rem / 60_000, rem % 60_000);
    let (s, ms) = (rem / 1000, rem % 1000);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_ms(0), "00:00:00.000");
        assert_eq!(format_ms(1_234), "00:00:01.234");
        assert_eq!(format_ms(61_005), "00:01:01.005");
        assert_eq!(
            format_ms(3 * 3_600_000 + 59 * 60_000 + 59_999),
            "03:59:59.999"
        );
        assert_eq!(format_ms(100 * 3_600_000), "100:00:00.000");
    }
}
