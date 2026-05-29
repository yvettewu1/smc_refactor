// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST10x0 SMC multi-CS mapped-read routing tests (QEMU-only).
//!
//! Verifies that a dual-CS FMC configuration exposes independent CS0 and CS1
//! mapped-read windows, and that requesting CS1 on a CS1-unconfigured
//! controller returns `SmcError::InvalidChipSelect`.
//!
//! QEMU provides a single `w25q80bl` model on CS0 of the FMC controller.
//! CS1 is configured in the HAL but the QEMU model does not attach a device
//! there — reads return undefined bytes. The test only asserts that mapped
//! reads complete without error.
//!
//! Tests (in order):
//!
//! 1. **Init** — construct FMC with cs0 + cs1 configured, run init.
//! 2. **CS capacity** — assert both configured CS slots report 1 MiB.
//! 3. **CS0 mapped read** — assert a device-local CS0 read succeeds.
//! 4. **CS1 mapped read** — assert a device-local CS1 read succeeds.
//! 5. **InvalidChipSelect guard** — build a CS1-unconfigured controller and
//!    assert a CS1 mapped read returns `InvalidChipSelect`.

#![no_std]
#![no_main]

use ast10x0_peripherals::smc::{
    ChipSelect, FlashConfig, FmcUninit, SmcConfig, SmcController, SmcError, SmcTopology,
};
use cortex_m_semihosting::debug::{exit, EXIT_FAILURE, EXIT_SUCCESS};
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

pub struct Target {}

const FLASH_CFG: FlashConfig = FlashConfig {
    capacity_mb: 1,
    page_size: 256,
    sector_size: 4096,
    block_size: 65536,
    spi_clock_mhz: 25,
};
const CS_PROBE_OFFSET: u32 = 0x000F_F000;

fn run_multi_cs_test() -> Result<(), SmcError> {
    // --- 1. Init with cs0 + cs1 both configured ---
    let config = SmcConfig {
        controller_id: SmcController::Fmc,
        cs0: Some(FLASH_CFG),
        cs1: Some(FLASH_CFG),
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };

    let uninit = unsafe { FmcUninit::new(config)? };
    let fmc = uninit.init()?;

    if !fmc.is_ready() {
        return Err(SmcError::HardwareError);
    }

    // --- 2. CS capacity ---
    if fmc.cs_capacity_bytes(ChipSelect::Cs0)? != 1024 * 1024
        || fmc.cs_capacity_bytes(ChipSelect::Cs1)? != 1024 * 1024
    {
        return Err(SmcError::HardwareError);
    }

    // --- 3. CS0 mapped read ---
    let mut cs0 = [0u8; 8];
    let cs0_n = fmc.read(ChipSelect::Cs0, CS_PROBE_OFFSET, &mut cs0)?;
    if cs0_n != cs0.len() {
        return Err(SmcError::HardwareError);
    }

    // --- 4. CS1 mapped read ---
    let mut cs1 = [0u8; 8];
    let cs1_n = fmc.read(ChipSelect::Cs1, CS_PROBE_OFFSET, &mut cs1)?;
    if cs1_n != cs1.len() {
        return Err(SmcError::HardwareError);
    }

    // --- 5. InvalidChipSelect guard: CS1-unconfigured controller ---
    let config_cs0_only = SmcConfig {
        controller_id: SmcController::Fmc,
        cs0: Some(FLASH_CFG),
        cs1: None,
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };
    let uninit2 = unsafe { FmcUninit::new(config_cs0_only)? };
    let fmc2 = uninit2.init()?;

    match fmc2.read(ChipSelect::Cs1, 0, &mut [0u8; 1]) {
        Err(SmcError::InvalidChipSelect) => {}
        other => {
            let _ = other;
            return Err(SmcError::HardwareError);
        }
    }

    Ok(())
}

declare_target!(Target);

impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 SMC Multi-CS Mapped Routing Test";

    fn main() -> ! {
        let exit_status = match run_multi_cs_test() {
            Ok(()) => EXIT_SUCCESS,
            Err(_) => EXIT_FAILURE,
        };
        exit(exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}
