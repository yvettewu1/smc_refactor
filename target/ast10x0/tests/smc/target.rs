// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST10x0 SMC portable smoke test target.
//!
//! Safe to run on both QEMU and silicon.  Does not assert on flash content
//! because silicon flash will not be in the erased state.
//!
//! Tests (in order):
//!
//! 1. **Init** — construct FMC controller, run hardware init, assert Ready.
//! 2. **PIO read — bounds rejection** — assert that a read past the configured
//!    capacity returns `SmcError::InvalidCapacity` before touching hardware.
//! 3. **DMA disabled rejection** — assert that `dma_read` returns
//!    `SmcError::DmaNotEnabled` when `SmcConfig::dma_enabled` is false.

#![no_std]
#![no_main]

use ast10x0_peripherals::smc::{
    ChipSelect, FlashConfig, SmcConfig, SmcController, SmcError, SmcTopology, UninitSmc,
};

use console_backend::console_backend_write_all;
use cortex_m_semihosting::debug::{exit, EXIT_FAILURE, EXIT_SUCCESS};
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

pub struct Target {}

fn run_smc_smoke_test() -> Result<(), SmcError> {
    // --- 1. Init ---

    let config = SmcConfig {
        controller_id: SmcController::Fmc,
        // winbond_w25q64 = 8 MB; QEMU's default w25q80bl is 1 MB.
        // Use a 1 MB config to stay inside the emulated chip boundary.
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

    // --- 2. PIO read — bounds rejection ---
    // 1 MB capacity = 0x10_0000 bytes.  Offset 0xFFFFF with len 8 crosses the
    // boundary; validate_mapped_range must reject it before any MMIO access.
    let mut overflow_buf = [0u8; 8];
    match controller.read(ChipSelect::Cs0, 0x000F_FFFF, &mut overflow_buf) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    }

    // --- 3. DMA disabled rejection ---
    // dma_enabled: false in the config above; dma_read must return
    // DmaNotEnabled before touching any hardware.  Argument validation
    // (alignment, bounds) is covered by unit tests in helpers.rs.
    match controller.dma_read(ChipSelect::Cs0, 0, 0x2, 256) {
        Err(SmcError::DmaNotEnabled) => Ok(()),
        Err(other) => Err(other),
        Ok(()) => Err(SmcError::HardwareError),
    }
}

impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 SMC Smoke Test";

    fn main() -> ! {
        let exit_status = match run_smc_smoke_test() {
            Ok(()) => EXIT_SUCCESS,
            Err(_e) => EXIT_FAILURE,
        };
        let sentinel: &[u8] = if exit_status == EXIT_SUCCESS {
            b"TEST_RESULT:PASS\n"
        } else {
            b"TEST_RESULT:FAIL\n"
        };
        let _ = console_backend_write_all(sentinel);
        exit(exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);
