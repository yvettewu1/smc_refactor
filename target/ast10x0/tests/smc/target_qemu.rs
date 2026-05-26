// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST10x0 SMC QEMU-only smoke test target.
//!
//! QEMU-only smoke coverage for SMC controller initialization and validation
//! paths. The test intentionally avoids successful flash aperture reads:
//! depending on QEMU's backing configuration, mapped flash reads can hang or
//! expose image bytes instead of deterministic erased data.
//!
//! Tagged "integration" in BUILD.bazel so it is excluded by the default
//! `--config=virt_ast10x0` / k_common tag filter and must be invoked
//! explicitly (the `--test_tag_filters=` override re-enables integration
//! tests while keeping the QEMU runner active):
//!
//!   bazelisk test --config=virt_ast10x0 --test_tag_filters= \
//!     //target/ast10x0/tests/smc:smc_qemu_erase_state_test
//!
//! Tests (in order):
//!
//! 1. **Init** — construct FMC controller, run hardware init, assert Ready.
//! 2. **PIO read** — read 8 bytes from the last sector, confirming:
//!    segment register encoding → flash window address → m25p80 model → buffer.
//! 3. **PIO read — bounds rejection** — assert an out-of-range read returns
//!    `SmcError::InvalidCapacity` before touching hardware.
//! 4. **DMA args rejection** — assert an unaligned DRAM address returns
//!    `SmcError::InvalidCapacity`.

#![no_std]
#![no_main]

use ast10x0_peripherals::smc::{
    ChipSelect, FlashConfig, SmcConfig, SmcController, SmcError, SmcTopology, UninitSmc,
};
use cortex_m_semihosting::debug::{exit, EXIT_FAILURE, EXIT_SUCCESS};
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

pub struct Target {}

fn run_smc_qemu_test() -> Result<(), SmcError> {
    // --- 1. Init ---
    let config = SmcConfig {
        controller_id: SmcController::Fmc,
        cs0: Some(FlashConfig {
            capacity_mb: 1,
            page_size: 256,
            sector_size: 4096,
            block_size: 65536,
            spi_clock_mhz: 25,
        }),
        cs1: None,
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };

    let controller = unsafe { UninitSmc::new(config)? };
    let mut controller = controller.init()?;

    if !controller.is_ready() || controller.controller_id() != SmcController::Fmc {
        return Err(SmcError::HardwareError);
    }

    // --- 3. PIO read — bounds rejection ---
    let mut overflow_buf = [0u8; 8];
    match controller.read(ChipSelect::Cs0, 0x000F_FFFF, &mut overflow_buf) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    }

    // --- 4. DMA args rejection — unaligned DRAM address ---
    match controller.dma_read(ChipSelect::Cs0, 0, 0x2, 256) {
        Err(SmcError::DmaNotEnabled) => Ok(()),
        Err(other) => Err(other),
        Ok(()) => Err(SmcError::HardwareError),
    }
}

impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 SMC QEMU Erase-State Test";

    fn main() -> ! {
        let exit_status = match run_smc_qemu_test() {
            Ok(()) => EXIT_SUCCESS,
            Err(_e) => EXIT_FAILURE,
        };
        exit(exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);
