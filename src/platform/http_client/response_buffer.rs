#![cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
    allow(dead_code)
)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseBodyReadPlan {
    Heap {
        initial_cap: usize,
    },
    PsramExact {
        cap: usize,
    },
    GrowThenPsram {
        initial_cap: usize,
        psram_initial_cap: usize,
        psram_max_cap: usize,
        switch_len: usize,
    },
}

pub(crate) fn choose_response_body_read_plan(
    max_len: usize,
    content_length_hint: Option<usize>,
    psram_supported: bool,
    initial_response_body_cap: usize,
    psram_prealloc_threshold: usize,
) -> ResponseBodyReadPlan {
    let hinted_len = content_length_hint.map(|len| len.min(max_len));
    if psram_supported {
        match hinted_len {
            Some(prealloc_len) if prealloc_len >= psram_prealloc_threshold => {
                return ResponseBodyReadPlan::PsramExact { cap: prealloc_len };
            }
            Some(seed_len) => {
                return ResponseBodyReadPlan::PsramExact {
                    cap: seed_len.max(initial_response_body_cap).min(max_len),
                };
            }
            None => {
                return ResponseBodyReadPlan::GrowThenPsram {
                    initial_cap: initial_response_body_cap.min(max_len),
                    psram_initial_cap: psram_prealloc_threshold
                        .saturating_mul(2)
                        .max(initial_response_body_cap)
                        .min(max_len),
                    psram_max_cap: max_len,
                    switch_len: psram_prealloc_threshold.min(max_len),
                };
            }
        }
    }

    let initial_cap = hinted_len.unwrap_or(initial_response_body_cap).min(max_len);
    ResponseBodyReadPlan::Heap { initial_cap }
}

#[cfg(test)]
mod tests {
    use super::{choose_response_body_read_plan, ResponseBodyReadPlan};

    #[test]
    fn unknown_length_starts_small_then_moves_to_psram_on_esp() {
        let plan = choose_response_body_read_plan(512 * 1024, None, true, 8 * 1024, 8 * 1024);
        assert_eq!(
            plan,
            ResponseBodyReadPlan::GrowThenPsram {
                initial_cap: 8 * 1024,
                psram_initial_cap: 16 * 1024,
                psram_max_cap: 512 * 1024,
                switch_len: 8 * 1024,
            }
        );
    }

    #[test]
    fn tiny_known_length_starts_in_psram_seeded_vec_on_esp() {
        let plan = choose_response_body_read_plan(512 * 1024, Some(512), true, 8 * 1024, 8 * 1024);
        assert_eq!(plan, ResponseBodyReadPlan::PsramExact { cap: 8 * 1024 });
    }

    #[test]
    fn large_known_length_keeps_exact_psram_prealloc_on_esp() {
        let plan =
            choose_response_body_read_plan(512 * 1024, Some(24 * 1024), true, 8 * 1024, 8 * 1024);
        assert_eq!(plan, ResponseBodyReadPlan::PsramExact { cap: 24 * 1024 });
    }

    #[test]
    fn unknown_length_without_psram_falls_back_to_heap() {
        let plan = choose_response_body_read_plan(512 * 1024, None, false, 8 * 1024, 8 * 1024);
        assert_eq!(
            plan,
            ResponseBodyReadPlan::Heap {
                initial_cap: 8 * 1024
            }
        );
    }
}
