pub fn should_trigger_compression(
    turn_completed: bool,
    at_turn_boundary: bool,
    turn_count: u32,
    next_compress_at: u32,
) -> bool {
    turn_completed && at_turn_boundary && turn_count >= next_compress_at
}

#[cfg(test)]
mod tests {
    use super::should_trigger_compression;

    #[test]
    fn triggers_only_at_boundary_and_threshold() {
        assert!(!should_trigger_compression(false, true, 5, 5));
        assert!(!should_trigger_compression(true, false, 5, 5));
        assert!(!should_trigger_compression(true, true, 4, 5));
        assert!(should_trigger_compression(true, true, 5, 5));
        assert!(should_trigger_compression(true, true, 8, 5));
    }
}
