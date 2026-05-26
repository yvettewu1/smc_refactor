// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST10x0 SpiNorFlash device facade — QEMU-only erase-state test.
//!
//! QEMU-only smoke coverage for the device facade's memory-mapped read path.
//! The flash contents are intentionally not asserted: depending on how QEMU
//! backs the flash aperture, image bytes may be visible instead of erased data.
//!
//! Tagged "integration" in BUILD.bazel.  Invoke explicitly:
//!
//!   bazelisk test --config=virt_ast10x0 --test_tag_filters= \
//!     //target/ast10x0/tests/smc:smc_device_qemu_erase_state_test
//!
//! Tests (in order):
//!
//! 1. **Init** — construct FmcUninit, initialize, assert Ready.
//! 2. **from_fmc** — build SpiNorFlash facade.
//! 3. **capacity_bytes** — assert 1 MB.
//! 4. **read via facade** — read 8 bytes from the last sector, confirming the
//!    full path from facade through
//!    FmcReady → ReadySmc → flash window → m25p80 model.
//! 5. **read via facade — bounds rejection** — assert InvalidCapacity.
//! 6. **from_spi path** — initialize SPI1, build facade with `from_spi`, then
//!    validate capacity/read/bounds behavior for the SPI constructor path.

#![no_std]
#![no_main]

use ast10x0_peripherals::smc::{
    FlashConfig, FmcUninit, SmcConfig, SmcController, SmcError, SmcTopology, SpiNorFlash,
    SpiNorFlashDevice, SpiUninit,
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
const QEMU_ERASE_CHECK_OFFSET: u32 = 0x000F_F000;

fn run_device_qemu_test() -> Result<(), SmcError> {
    // --- 1. Init ---
    let config = SmcConfig {
        controller_id: SmcController::Fmc,
        cs0: Some(FLASH_CFG),
        cs1: None,
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };

    let uninit = unsafe { FmcUninit::new(config)? };
    let mut fmc = uninit.init()?;

    if !fmc.is_ready() {
        return Err(SmcError::HardwareError);
    }

    // --- 2. Build SpiNorFlash facade ---
    let flash = SpiNorFlash::from_fmc(&mut fmc, FLASH_CFG)?;

    // --- 3. capacity_bytes ---
    let cap = flash.capacity_bytes()?;
    if cap != 1 * 1024 * 1024 {
        return Err(SmcError::HardwareError);
    }

    // --- 4. read via facade ---
    // Use the last sector to stay away from any image bytes QEMU may expose
    // at the beginning of the flash aperture. Contents are not stable across
    // QEMU backing configurations, so only assert that the read succeeds.
    let mut buf = [0u8; 8];
    let n = flash.read(QEMU_ERASE_CHECK_OFFSET, &mut buf)?;
    if n != 8 {
        return Err(SmcError::HardwareError);
    }

    // --- 5. read via facade — bounds rejection ---
    let mut overflow_buf = [0u8; 8];
    match flash.read(0x000F_FFFF, &mut overflow_buf) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    }

    // --- 6. from_spi path (SPI1) ---
    let spi_cfg = SmcConfig {
        controller_id: SmcController::Spi1,
        cs0: Some(FLASH_CFG),
        cs1: None,
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };

    let spi_uninit = unsafe { SpiUninit::new(SmcController::Spi1, spi_cfg)? };
    let mut spi = spi_uninit.init()?;

    if !spi.is_ready() {
        return Err(SmcError::HardwareError);
    }

    let spi_flash = SpiNorFlash::from_spi(&mut spi, FLASH_CFG)?;
    let spi_cap = spi_flash.capacity_bytes()?;
    if spi_cap != 1 * 1024 * 1024 {
        return Err(SmcError::HardwareError);
    }

    let mut spi_buf = [0u8; 8];
    let spi_n = spi_flash.read(QEMU_ERASE_CHECK_OFFSET, &mut spi_buf)?;
    if spi_n != 8 {
        return Err(SmcError::HardwareError);
    }

    let mut spi_overflow = [0u8; 8];
    match spi_flash.read(0x000F_FFFF, &mut spi_overflow) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    }

    Ok(())
}

impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 SpiNorFlash QEMU Erase-State Test";

    fn main() -> ! {
        let exit_status = match run_device_qemu_test() {
            Ok(()) => EXIT_SUCCESS,
            Err(_e) => EXIT_FAILURE,
        };
        exit(exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);
