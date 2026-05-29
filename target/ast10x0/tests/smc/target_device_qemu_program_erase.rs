// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

//! AST10x0 SpiNorFlash device facade — QEMU command-profile smoke test.
//!
//! QEMU's AST10x0 FMC command-mode path is not reliable enough here for a
//! mutating program/erase integration test. This target keeps QEMU coverage on
//! the parts that are deterministic under the runner: facade construction,
//! capacity, mapped reads, bounds checks, and opcode-profile selection.

#![no_std]
#![no_main]

use ast10x0_peripherals::smc::{
    AddressWidth, FlashAddressingPolicy, FlashConfig, FmcUninit, SmcConfig, SmcController,
    SmcError, SmcTopology, SpiNorFlash, SpiNorFlashDevice,
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
const QEMU_PROBE_OFFSET: u32 = 0x000F_F000;

fn run_device_program_erase_test() -> Result<(), SmcError> {
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

    let flash = SpiNorFlash::from_fmc(&mut fmc, FLASH_CFG)?;
    if flash.capacity_bytes()? != 1024 * 1024 {
        return Err(SmcError::HardwareError);
    }

    let mut mapped_probe = [0u8; 16];
    let read_len = flash.read(QEMU_PROBE_OFFSET, &mut mapped_probe)?;
    if read_len != mapped_probe.len() {
        return Err(SmcError::HardwareError);
    }

    let mut overflow = [0u8; 8];
    match flash.read(0x000F_FFFF, &mut overflow) {
        Err(SmcError::InvalidCapacity) => {}
        Err(other) => return Err(other),
        Ok(_) => return Err(SmcError::HardwareError),
    }

    let flash_4b = SpiNorFlash::from_fmc(&mut fmc, FLASH_CFG)?
        .with_addressing_policy(FlashAddressingPolicy::FourByteCommands);
    if flash_4b.addr_width() != AddressWidth::FourByte {
        return Err(SmcError::HardwareError);
    }
    let profile = flash_4b.command_profile();
    if profile.page_program != 0x12 || profile.erase_sector_4k != 0x21 {
        return Err(SmcError::HardwareError);
    }

    Ok(())
}

impl TargetInterface for Target {
    const NAME: &'static str = "AST10x0 SpiNorFlash QEMU Program/Erase Test";

    fn main() -> ! {
        let exit_status = match run_device_program_erase_test() {
            Ok(()) => EXIT_SUCCESS,
            Err(_e) => EXIT_FAILURE,
        };
        exit(exit_status);
        #[expect(clippy::empty_loop)]
        loop {}
    }
}

declare_target!(Target);
