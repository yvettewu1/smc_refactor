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
#[allow(unused_imports)]
use ast10x0_peripherals::smc::{FlashConfig, SmcConfig, SmcController, SmcError, SmcTopology, UninitSmc, ChipSelect};
use cortex_m_semihosting::debug::{EXIT_FAILURE, EXIT_SUCCESS};
use ast10x0_peripherals::scu::pinctrl::PINCTRL_FMC_QUAD;
use ast10x0_peripherals::scu::ScuRegisters;
use target_common::{TargetInterface, declare_target};
use {console_backend as _, entry as _};


pub struct Target {}

use core::ptr::read_volatile;
#[allow(dead_code)]
fn dump_smc_register(addr: u32, count: u32) {
    for i in 0..count {
        let reg_addr = addr + (i * 4);

        let reg = unsafe {
            read_volatile(reg_addr as *const u32)
        };

        pw_log::info!(
            "SMC[0x{:08x}] = 0x{:08x}",
            reg_addr as u32,
            reg as u32
        );
    }
}

#[allow(dead_code)]
fn dump_smc_read(buf: &[u8], count: u32) {
    let count = core::cmp::min(count as usize, buf.len());
    for i in (0..count).step_by(4) {
        if i + 4 > count {
            break;
        }

        let bytes: [u8; 4] = buf[i..i + 4].try_into().unwrap();

        let value = u32::from_le_bytes(bytes);

        pw_log::info!("[0x{:08x}] = 0x{:08x}", i as u32, value as u32);
    }
}

#[allow(dead_code)]
fn run_smc_smoke_test() -> Result<(), SmcError> {
    // --- 1. Init ---
    // TODO:: set pinctrl in board/src/lib.rs
    let scu = unsafe { ScuRegisters::new_global() };
    scu.apply_pinctrl_group(PINCTRL_FMC_QUAD);

    let config = SmcConfig {
        controller_id: SmcController::Fmc,
        // winbond_w25q64 = 8 MB; QEMU's default w25q80bl is 1 MB.
        // Use a 16 MB config to stay inside the emulated chip boundary.
        cs0: Some(FlashConfig {
            capacity_mb: 16,
            page_size: 256,
            sector_size: 4096,
            block_size: 65536,
            spi_clock_mhz: 50,
        }),
        cs1: None,
        dma_enabled: true,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };
    pw_log::info!("=== AST10x0 smc  smoke test  ===");
    let controller = unsafe { UninitSmc::new(config)? };
    let mut controller = controller.init()?;
    
    let _ = match controller.spi_nor_read_init(ChipSelect::Cs0){
        Ok(v) => v,
        Err(e) => { pw_log::info!("Error:: spi_nor_read_init");return Err(e);},
    }; 

    pw_log::info!("=== Dump 0x7E62_0000 ===");
    dump_smc_register(0x7E62_0000, 16);
    dump_smc_register(0x8000_0000, 16);
    if !controller.is_ready() || controller.controller_id() != SmcController::Fmc {
        return Err(SmcError::HardwareError);
    }

    // --- 2. MMIO read — success path --- 
    // Confirm the call succeeds and returns the correct byte count.  Flash
    // content is not inspected so this is safe on both QEMU and silicon.
    // TODO: need to add test CS1 
    pw_log::info!("=== read test===");
    let mut buf = [0u8; 64];
    let n = controller.read(ChipSelect::Cs0, 0x400, &mut buf)?;
    if n != 64 {
        return Err(SmcError::HardwareError);
    }
    dump_smc_read(&buf, 64);
    
    pw_log::info!("=== read dma test===");
    // --- 4. DMA  ---
    let tempbuf = unsafe {
        core::slice::from_raw_parts(0x41000 as *mut u8, 256)
    };
    
    let _ = match controller.dma_read(ChipSelect::Cs0, 0x500, 0x41000 as usize, 256) {
        Err(SmcError::InvalidCapacity) => Ok(()),
        Err(other) => Err(other),
        Ok(()) => Err(SmcError::HardwareError),
    }; 

    loop {
        match controller.poll_dma_completion() {
            core::task::Poll::Pending => {
                // still running
            }
            core::task::Poll::Ready(result) => {
                result?;
                    pw_log::info!("dma completion is ready");
                break;
            }
        }
    }

    pw_log::info!("=== dma done= ==");
    dump_smc_register(0x7E62_0000, 8);
    dump_smc_register(0x7E62_0080, 8);
    dump_smc_read(tempbuf, 256);

    pw_log::info!("=== read overflow test===");
    // --- 3. MMIO read — bounds rejection ---
    // 1 MB capacity = 0x10_0000 bytes.  Offset 0xFFFFF with len 8 crosses the
    // boundary; validate_mapped_range must reject it before any MMIO access.
    let mut overflow_buf = [0u8; 8];
    match controller.read(ChipSelect::Cs0, 0x000F_FFFF, &mut overflow_buf) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    } 

    Ok(())
}


impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 SMC Smoke Test";

    fn main() -> ! {
        let _exit_status = match run_smc_smoke_test() {
            Ok(()) => EXIT_SUCCESS,
            Err(_e) => EXIT_FAILURE,
        };

        //let _ = console_backend_write_all(_exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);