// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST10x0 Flash Service Target — dual-CS variant.
//!
//! Identical to `target.rs` except the system codegen module bound is
//! `codegen_dual_cs`, matching the dual-CS system image's process layout.

#![no_std]
#![no_main]

use ast10x0_board::{apply_spim_wiring, presets, SpimWiring};
use ast10x0_peripherals::scu::ScuRegisters;
use ast10x0_peripherals::smc::SmcController;
use ast10x0_peripherals::spimonitor::MonitorPolicy;
use codegen_dual_cs as codegen;
use console_backend::console_backend_write_all;
use cortex_m_semihosting::debug::{exit, EXIT_FAILURE, EXIT_SUCCESS};
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

static BMC_POLICY: MonitorPolicy = presets::bmc_default_policy();

pub struct Target {}

impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 Flash Service (dual-CS)";

    fn main() -> ! {
        // SAFETY: kernel `main` runs before any user-space process is
        // started, so we have exclusive access to SCU and SPIPF.
        let scu = unsafe { ScuRegisters::new_global() };

        // SAFETY: see above.
        if unsafe {
            apply_spim_wiring(
                &scu,
                SmcController::Spi1,
                SpimWiring::default_spi1_via_spim0(),
                &BMC_POLICY,
            )
        }
        .is_err()
        {
            exit(EXIT_FAILURE);
        }

        // SAFETY: see above.
        if unsafe {
            apply_spim_wiring(
                &scu,
                SmcController::Spi2,
                SpimWiring::default_spi2_via_spim2(),
                &BMC_POLICY,
            )
        }
        .is_err()
        {
            exit(EXIT_FAILURE);
        }

        codegen::start();
        #[expect(clippy::empty_loop)]
        loop {}
    }

    fn shutdown(code: u32) -> ! {
        let sentinel: &[u8] = if code == 0 {
            b"TEST_RESULT:PASS\n"
        } else {
            b"TEST_RESULT:FAIL\n"
        };
        let _ = console_backend_write_all(sentinel);
        let status = if code == 0 {
            EXIT_SUCCESS
        } else {
            EXIT_FAILURE
        };
        exit(status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);
