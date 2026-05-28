// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST1060 EVB FMC flash-backend read test.
//!
//! Physical-board smoke coverage:
//! 1. Build the FMC backend from `ast1060_evb_fmc_aspeed_rust_derived()`.
//! 2. Read CS0/CS1 through the `flash/lib.rs` backend API.

#![no_std]
#![no_main]

use ast10x0_board::Ast10x0BoardDescriptor;
use ast10x0_peripherals::scu::ScuRegisters;
use console_backend::console_backend_write_all;
use flash_api::backend::{BackendError, FlashBackend};
use flash_backend::{Backend, Cs};
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

#[path = "../smc/target_debug.rs"]
mod target_debug;
use target_debug::{dump_smc_read, dump_smc_register};

const READ_OFFSET: u32 = 0x400;
const READ_LEN: usize = 64;
pub struct Target {}

fn run_fmc_read_test() -> Result<(), BackendError> {
    let descriptor = Ast10x0BoardDescriptor::ast1060_evb_fmc_aspeed_rust_derived();

    let scu = unsafe { ScuRegisters::new_global() };
    unsafe { descriptor.init_board(&scu) }.map_err(|_| BackendError::InternalError)?;

    pw_log::info!("=== AST10x0 FMC backend read test ===");
    let mut backend =
        Backend::new_with_descriptor(descriptor).map_err(|_| BackendError::InternalError)?;

    pw_log::info!("=== Dump 0x7E62_0000 ===");
    dump_smc_register(0x7E62_0000, 16);
    dump_smc_register(0x8000_0000, 16);

    let mut cs0_readback = [0u8; READ_LEN];
    pw_log::info!("=== read test cs0===");
    let cs0_n = backend.read(Cs::Cs0, READ_OFFSET, &mut cs0_readback)?;
    if cs0_n != READ_LEN {
        return Err(BackendError::InternalError);
    }
    dump_smc_read(&cs0_readback, READ_LEN as u32);

    let mut cs1_readback = [0u8; READ_LEN];
    pw_log::info!("=== read test cs1===");
    let cs1_n = backend.read(Cs::Cs1, READ_OFFSET, &mut cs1_readback)?;
    if cs1_n != READ_LEN {
        return Err(BackendError::InternalError);
    }
    dump_smc_read(&cs1_readback, READ_LEN as u32);

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
