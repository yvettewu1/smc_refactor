// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST1060 EVB FMC SMC read test.
//!
//! Physical-board smoke coverage:
//! 1. Configure FMC CS0/CS1 decode windows.
//! 2. Initialize FMC and program normal-read mode for CS0/CS1.
//! 3. Perform mapped SMC reads from CS0/CS1 and a DMA read from CS0.

#![no_std]
#![no_main]

use ast10x0_peripherals::scu::pinctrl::PINCTRL_FMC_QUAD;
use ast10x0_peripherals::scu::ScuRegisters;
use ast10x0_peripherals::smc::{
    ChipSelect, FlashConfig, SmcConfig, SmcController, SmcError, SmcTopology, UninitSmc,
};
use console_backend::console_backend_write_all;
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

#[path = "../smc/target_debug.rs"]
mod target_debug;
use target_debug::{dump_smc_read, dump_smc_register};

const READ_OFFSET: u32 = 0x400;
const READ_LEN: usize = 64;

pub struct Target {}

fn run_fmc_read_test() -> Result<(), SmcError> {
    let scu = unsafe { ScuRegisters::new_global() };
    scu.apply_pinctrl_group(PINCTRL_FMC_QUAD);

    let config = SmcConfig {
        controller_id: SmcController::Fmc,
        cs0: Some(FlashConfig {
            capacity_mb: 8,
            page_size: 256,
            sector_size: 4096,
            block_size: 65536,
            spi_clock_mhz: 50,
        }),
        cs1: Some(FlashConfig {
            capacity_mb: 64,
            page_size: 256,
            sector_size: 4096,
            block_size: 65536,
            spi_clock_mhz: 50,
        }),
        dma_enabled: true,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };

    pw_log::info!("=== AST10x0 smc flash read test ===");
    let controller = unsafe { UninitSmc::new(config)? };
    let mut controller = controller.init()?;

    controller.spi_nor_read_init(ChipSelect::Cs0)?;
    controller.spi_nor_read_init(ChipSelect::Cs1)?;

    pw_log::info!("=== Dump 0x7E62_0000 ===");
    dump_smc_register(0x7E62_0000, 16);
    dump_smc_register(0x8000_0000, 16);
    if !controller.is_ready() || controller.controller_id() != SmcController::Fmc {
        return Err(SmcError::HardwareError);
    }

    let mut cs0_readback = [0u8; READ_LEN];
    pw_log::info!("=== read test cs0===");
    let cs0_n = controller.read(ChipSelect::Cs0, READ_OFFSET, &mut cs0_readback)?;
    if cs0_n != READ_LEN {
        return Err(SmcError::HardwareError);
    }
    dump_smc_read(&cs0_readback, READ_LEN as u32);

    let mut cs1_readback = [0u8; READ_LEN];
    pw_log::info!("=== read test cs1===");
    let cs1_n = controller.read(ChipSelect::Cs1, READ_OFFSET, &mut cs1_readback)?;
    if cs1_n != READ_LEN {
        return Err(SmcError::HardwareError);
    }
    dump_smc_read(&cs1_readback, READ_LEN as u32);

    pw_log::info!("=== read dma test===");
    let tempbuf = unsafe { core::slice::from_raw_parts(0x41000 as *mut u8, 256) };

    controller.dma_read(ChipSelect::Cs0, 0x500, 0x41000 as usize, 256)?;

    loop {
        match controller.poll_dma_completion() {
            core::task::Poll::Pending => {}
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

    Ok(())
}

impl TargetInterface for Target {
    const NAME: &'static str = "AST1060 EVB FMC derived SMC read test";

    fn main() -> ! {
        let sentinel: &[u8] = if run_fmc_read_test().is_ok() {
            b"TEST_RESULT:PASS\n"
        } else {
            b"TEST_RESULT:FAIL\n"
        };
        let _ = console_backend_write_all(sentinel);

        // Physical-board UART tests stop after the sentinel. Semihosting exit
        // faults on silicon when no debugger handles the BKPT request.
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);
