// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST1060 EVB FMC CS1 erase/write/readback verify test.
//!
//! Physical-board coverage:
//! 1. Initialize the board from `ast1060_evb_fmc_aspeed_rust_derived()`.
//! 2. Construct `Ast10x0FlashBackend` from that descriptor.
//! 3. Back up one CS1 sector, erase it, verify erased bytes, write one page,
//!    read the page back, verify it, then restore the original sector.

#![no_std]
#![no_main]

use ast10x0_board::Ast10x0BoardDescriptor;
use ast10x0_peripherals::scu::ScuRegisters;
use console_backend::console_backend_write_all;
use core::cell::UnsafeCell;
use flash_backend::{Backend, BackendError, Cs};
use target_common::{declare_target, TargetInterface};
use {console_backend as _, entry as _};

const TEST_OFFSET: u32 = 0x0010_0000;
const SECTOR_SIZE: usize = 4096;
const PAGE_SIZE: usize = 256;

struct TestBuffers {
    backup: [u8; SECTOR_SIZE],
    page: [u8; PAGE_SIZE],
    readback: [u8; PAGE_SIZE],
}

struct SharedBuffers(UnsafeCell<TestBuffers>);

// The target test runs once on the initial kernel thread, so these buffers are
// never accessed concurrently.
unsafe impl Sync for SharedBuffers {}

static BUFFERS: SharedBuffers = SharedBuffers(UnsafeCell::new(TestBuffers {
    backup: [0; SECTOR_SIZE],
    page: [0; PAGE_SIZE],
    readback: [0; PAGE_SIZE],
}));

pub struct Target {}

fn read_exact(backend: &mut Backend, address: u32, out: &mut [u8]) -> Result<(), BackendError> {
    let mut offset = 0usize;
    while offset < out.len() {
        let end = core::cmp::min(offset + PAGE_SIZE, out.len());
        let n = backend.read(Cs::Cs1, address + offset as u32, &mut out[offset..end])?;
        if n != end - offset {
            return Err(BackendError::InternalError);
        }
        offset = end;
    }
    Ok(())
}

fn write_pages(backend: &mut Backend, address: u32, data: &[u8]) -> Result<(), BackendError> {
    let mut offset = 0usize;
    while offset < data.len() {
        let end = core::cmp::min(offset + PAGE_SIZE, data.len());
        let n = backend.write(Cs::Cs1, address + offset as u32, &data[offset..end])?;
        if n != end - offset {
            return Err(BackendError::InternalError);
        }
        offset = end;
    }
    Ok(())
}

fn fill_pattern(page: &mut [u8; PAGE_SIZE]) {
    for (idx, byte) in page.iter_mut().enumerate() {
        *byte = 0xA5 ^ (idx as u8).wrapping_mul(17);
    }
}

fn verify_erased_sector(
    backend: &mut Backend,
    readback: &mut [u8; PAGE_SIZE],
) -> Result<(), BackendError> {
    for offset in (0..SECTOR_SIZE).step_by(PAGE_SIZE) {
        read_exact(backend, TEST_OFFSET + offset as u32, readback)?;
        if !readback.iter().all(|byte| *byte == 0xFF) {
            return Err(BackendError::IoError);
        }
    }
    Ok(())
}

fn exercise_sector(backend: &mut Backend, buffers: &mut TestBuffers) -> Result<(), BackendError> {
    pw_log::info!("cs1 erase sector start");
    backend.erase(Cs::Cs1, TEST_OFFSET, SECTOR_SIZE as u32)?;

    pw_log::info!("cs1 verify erased start");
    verify_erased_sector(backend, &mut buffers.readback)?;

    fill_pattern(&mut buffers.page);
    pw_log::info!("cs1 write page start");
    write_pages(backend, TEST_OFFSET, &buffers.page)?;

    pw_log::info!("cs1 readback verify start");
    read_exact(backend, TEST_OFFSET, &mut buffers.readback)?;
    if buffers.readback != buffers.page {
        return Err(BackendError::IoError);
    }

    Ok(())
}

fn restore_sector(backend: &mut Backend, backup: &[u8; SECTOR_SIZE]) -> Result<(), BackendError> {
    pw_log::info!("cs1 restore sector start");
    backend.erase(Cs::Cs1, TEST_OFFSET, SECTOR_SIZE as u32)?;
    write_pages(backend, TEST_OFFSET, backup)
}

fn run_fmc_cs1_erase_write_verify() -> Result<(), BackendError> {
    let descriptor = Ast10x0BoardDescriptor::ast1060_evb_fmc_aspeed_rust_derived();

    // SAFETY: this kernel-only board test initializes board pinctrl before any
    // competing flash owner exists.
    let scu = unsafe { ScuRegisters::new_global() };
    unsafe { descriptor.init_board(&scu) }.map_err(|_| BackendError::InternalError)?;

    pw_log::info!("=== AST10x0 FMC CS1 erase/write/readback verify test ===");
    let mut backend =
        Backend::new_with_descriptor(descriptor).map_err(|_| BackendError::InternalError)?;

    let info = backend.info(Cs::Cs1);
    pw_log::info!(
        "cs1 info capacity={} erase={} chunk={}",
        info.capacity as u32,
        info.erase_size as u32,
        info.chunk_size as u32
    );


    if info.capacity < TEST_OFFSET + SECTOR_SIZE as u32
        || info.erase_size != SECTOR_SIZE as u32
        || info.chunk_size != PAGE_SIZE as u32
    {
        return Err(BackendError::InvalidLength);
    }

    // SAFETY: single-threaded test execution, guarded by the static Sync
    // invariant above.
    let buffers = unsafe { &mut *BUFFERS.0.get() };

    pw_log::info!("cs1 backup sector start");
    read_exact(&mut backend, TEST_OFFSET, &mut buffers.backup)?;

    let test_result = exercise_sector(&mut backend, buffers);
    let restore_result = restore_sector(&mut backend, &buffers.backup);
    test_result?;
    restore_result?;

    pw_log::info!("cs1 erase/write/readback verify passed");
    Ok(())
}

impl TargetInterface for Target {
    const NAME: &'static str = "AST1060 EVB FMC CS1 erase write verify test";

    fn main() -> ! {
        let sentinel: &[u8] = if run_fmc_cs1_erase_write_verify().is_ok() {
            b"TEST_RESULT:PASS\n"
        } else {
            b"TEST_RESULT:FAIL\n"
        };
        let _ = console_backend_write_all(sentinel);

        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);