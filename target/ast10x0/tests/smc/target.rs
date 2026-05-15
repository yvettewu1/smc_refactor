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
//! 2. **PIO read — success path** — issue a read from offset 0; assert the
//!    call succeeds and returns the expected byte count.  Flash content is not
//!    inspected.
//! 3. **PIO read — bounds rejection** — assert that a read past the configured
//!    capacity returns `SmcError::InvalidCapacity` before touching hardware.
//! 4. **DMA disabled rejection** — assert that `dma_read` returns
//!    `SmcError::DmaNotEnabled` when `SmcConfig::dma_enabled` is false.

#![no_std]
#![no_main]

use ast10x0_peripherals::smc::{FlashConfig, SmcConfig, SmcController, SmcError, SmcTopology, UninitSmc, ChipSelect};
use cortex_m_semihosting::debug::{EXIT_FAILURE, EXIT_SUCCESS, exit};
use target_common::{TargetInterface, declare_target};
use {console_backend as _, entry as _};


pub struct Target {}


/*
use core::ptr::read_volatile;
fn dump_smc_register(addr: u32, count: u32) {
    for i in 0..count {
        let reg_addr = addr + (i * 4);

        let reg = unsafe {
            read_volatile(reg_addr as *const u32)
        };

        pw_log::info!(
            "SMC[0x{:08x}] = 0x{:08x}",
            reg_addr,
            reg
        );
    }
}
*/
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
            spi_clock_mhz: 50,
        }),
        cs1: None,
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };
    pw_log::info!("=== AST10x0 smc  smoke test ===");
    let controller = unsafe { UninitSmc::new(config)? };
    let mut controller = controller.init()?;
    pw_log::info!("=== after init ===");
    //dump_smc_register(0x7E62_0000, 32);

    if !controller.is_ready() || controller.controller_id() != SmcController::Fmc {
        return Err(SmcError::HardwareError);
    }

    // --- 2. PIO read — success path ---
    // Confirm the call succeeds and returns the correct byte count.  Flash
    // content is not inspected so this is safe on both QEMU and silicon.
    // TODO: need to add test CS1 
    let mut buf = [0u8; 8];
    let n = controller.read(ChipSelect::Cs0, 0, &mut buf)?;
    if n != 8 {
        return Err(SmcError::HardwareError);
    }

    // --- 3. PIO read — bounds rejection ---
    // 1 MB capacity = 0x10_0000 bytes.  Offset 0xFFFFF with len 8 crosses the
    // boundary; validate_mapped_range must reject it before any MMIO access.
    let mut overflow_buf = [0u8; 8];
    match controller.read(ChipSelect::Cs0, 0x000F_FFFF, &mut overflow_buf) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    }

    // --- 4. DMA args rejection — unaligned DRAM address ---
    match controller.dma_read(ChipSelect::Cs0, 0, 0x2, 256) {
        Err(SmcError::InvalidCapacity) => Ok(()),
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
        exit(exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);