pub(crate) fn whole_record_lease_advanced<T: PartialEq>(
    baseline: Option<&T>,
    latest: Option<&T>,
    baseline_updated_at: u64,
    latest_updated_at: u64,
) -> bool {
    baseline != latest && latest_updated_at > baseline_updated_at
}
