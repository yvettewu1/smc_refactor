// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

#![no_std]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use ast10x0_board::{apply_spim_wiring, Ast10x0BoardDescriptor, SpimWiringError};
use ast10x0_peripherals::scu::ScuRegisters;
use ast10x0_peripherals::smc::{
    ChipSelect, FlashConfig, FmcReady, FmcUninit, SmcConfig, SmcController, SmcError, SmcTopology,
    SpiNorFlash, SpiNorFlashDevice, SpiReady, SpiUninit,
};

/// Re-export so test binaries can name the route key without taking a
/// separate dependency on `ast10x0_peripherals`.
pub use ast10x0_peripherals::smc::ChipSelect as Cs;
use ast10x0_peripherals::spimonitor::LockedSpiMonitor;

/// Errors raised while constructing the flash backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendInitError {
    /// Descriptor did not provide a CS0 flash configuration.
    MissingCs0Config,
    /// FMC controller cannot have SPIM wiring; descriptor must set `spim_wiring: None`.
    FmcWithSpimWiring,
    /// SPI controller requires SPIM wiring; descriptor must set `spim_wiring: Some(_)`.
    SpiWithoutSpimWiring,
    /// Applying SCU/SPIPF wiring failed.
    SpimWiring(SpimWiringError),
    /// SMC controller construction or init failed.
    Smc(SmcError),
}

impl From<SpimWiringError> for BackendInitError {
    fn from(value: SpimWiringError) -> Self {
        Self::SpimWiring(value)
    }
}

impl From<SmcError> for BackendInitError {
    fn from(value: SmcError) -> Self {
        Self::Smc(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    InvalidOperation,
    InvalidAddress,
    InvalidLength,
    BufferTooSmall,
    Busy,
    Timeout,
    IoError,
    NotPermitted,
    InternalError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashInfo {
    pub capacity: u32,
    pub chunk_size: u32,
    pub erase_size: u32,
}

fn smc_to_backend_error(err: SmcError) -> BackendError {
    match err {
        SmcError::InvalidChipSelect => BackendError::InvalidAddress,
        SmcError::InvalidCapacity => BackendError::InvalidLength,
        SmcError::WriteProtected => BackendError::NotPermitted,
        SmcError::ControllerNotReady => BackendError::Busy,
        SmcError::Timeout => BackendError::Timeout,
        SmcError::DmaAborted | SmcError::DmaLengthMismatch => BackendError::IoError,
        SmcError::DeviceNotSupported
        | SmcError::WriteInProgress
        | SmcError::DmaNotEnabled
        | SmcError::HardwareError => BackendError::InternalError,
    }
}

pub struct Ast10x0FlashBackend {
    controller: ControllerBackend,
    cs0_cfg: FlashConfig,
    cs1_cfg: Option<FlashConfig>,
    /// SPIPF lock witness held for the lifetime of the backend. `None` for
    /// FMC; `Some(_)` for SPI controllers. Dropping does not unlock; the
    /// SPIPF lock is one-way per silicon spec.
    _monitor: Option<LockedSpiMonitor>,
}

enum ControllerBackend {
    Fmc(FmcReady),
    Spi(SpiReady),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ast10x0Controller {
    Fmc,
    Spi1,
    Spi2,
}

impl Ast10x0FlashBackend {
    /// Construct backend for the FMC controller with the default flash config.
    pub fn new() -> Result<Self, BackendInitError> {
        Self::new_for_controller(Ast10x0Controller::Fmc)
    }

    /// Construct an FMC-backed backend.
    pub fn new_fmc() -> Result<Self, BackendInitError> {
        Self::new_for_controller(Ast10x0Controller::Fmc)
    }

    /// Construct an SPI1-backed backend with default SPIM0 wiring and the
    /// BMC default opcode allow-list policy.
    pub fn new_spi1() -> Result<Self, BackendInitError> {
        Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi1())
    }

    /// Construct an SPI2-backed backend with default SPIM2 wiring and the
    /// BMC default opcode allow-list policy.
    pub fn new_spi2() -> Result<Self, BackendInitError> {
        Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi2())
    }

    /// Construct an FMC-backed backend exposing both CS0 and CS1.
    pub fn new_fmc_dual_cs() -> Result<Self, BackendInitError> {
        Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_dual_cs())
    }

    /// Construct an SPI1-backed dual-CS backend.
    pub fn new_spi1_dual_cs() -> Result<Self, BackendInitError> {
        Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi1_dual_cs())
    }

    /// Construct an SPI2-backed dual-CS backend.
    pub fn new_spi2_dual_cs() -> Result<Self, BackendInitError> {
        Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi2_dual_cs())
    }

    /// SPI1-backed dual-CS backend assuming kernel-side pre-wiring.
    pub fn new_spi1_dual_cs_pre_wired() -> Result<Self, BackendInitError> {
        Self::new_with_pre_wired_descriptor(
            Ast10x0BoardDescriptor::ast10x0_qemu_default_spi1_dual_cs(),
        )
    }

    /// SPI2-backed dual-CS backend assuming kernel-side pre-wiring.
    pub fn new_spi2_dual_cs_pre_wired() -> Result<Self, BackendInitError> {
        Self::new_with_pre_wired_descriptor(
            Ast10x0BoardDescriptor::ast10x0_qemu_default_spi2_dual_cs(),
        )
    }

    /// Construct a backend for the requested controller using a built-in
    /// default descriptor.
    pub fn new_for_controller(controller: Ast10x0Controller) -> Result<Self, BackendInitError> {
        match controller {
            Ast10x0Controller::Fmc => {
                Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default())
            }
            Ast10x0Controller::Spi1 => {
                Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi1())
            }
            Ast10x0Controller::Spi2 => {
                Self::new_with_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi2())
            }
        }
    }

    /// Construct a backend from a board descriptor.
    pub fn new_with_descriptor(
        descriptor: Ast10x0BoardDescriptor,
    ) -> Result<Self, BackendInitError> {
        let cs0_cfg = descriptor.cs0.ok_or(BackendInitError::MissingCs0Config)?;
        let cs1_cfg = descriptor.cs1;

        let monitor = match (descriptor.controller, descriptor.spim_wiring.as_ref()) {
            (SmcController::Fmc, None) => None,
            (SmcController::Fmc, Some(_)) => {
                return Err(BackendInitError::FmcWithSpimWiring);
            }
            (SmcController::Spi1, None) | (SmcController::Spi2, None) => {
                return Err(BackendInitError::SpiWithoutSpimWiring);
            }
            (controller_id, Some(wiring)) => {
                // SAFETY: the backend takes exclusive ownership of the SCU
                // block and the routed SPIPF block for its lifetime; one
                // backend instance exists per server process.
                let scu = unsafe { ScuRegisters::new_global() };
                let locked = unsafe {
                    apply_spim_wiring(&scu, controller_id, *wiring, &descriptor.monitor_policy)
                }?;
                Some(locked)
            }
        };

        let controller = build_smc_controller(descriptor.controller, cs0_cfg, cs1_cfg)?;
        Ok(Self {
            controller,
            cs0_cfg,
            cs1_cfg,
            _monitor: monitor,
        })
    }

    /// Construct a backend assuming SPIM wiring + SPIPF policy + SPIPF
    /// lock have already been programmed by trusted setup code.
    pub fn new_with_pre_wired_descriptor(
        descriptor: Ast10x0BoardDescriptor,
    ) -> Result<Self, BackendInitError> {
        let cs0_cfg = descriptor.cs0.ok_or(BackendInitError::MissingCs0Config)?;
        let cs1_cfg = descriptor.cs1;

        match (descriptor.controller, descriptor.spim_wiring.as_ref()) {
            (SmcController::Fmc, None) => {}
            (SmcController::Fmc, Some(_)) => {
                return Err(BackendInitError::FmcWithSpimWiring);
            }
            (SmcController::Spi1, None) | (SmcController::Spi2, None) => {
                return Err(BackendInitError::SpiWithoutSpimWiring);
            }
            (_, Some(_)) => {}
        }

        let controller = build_smc_controller(descriptor.controller, cs0_cfg, cs1_cfg)?;
        Ok(Self {
            controller,
            cs0_cfg,
            cs1_cfg,
            _monitor: None,
        })
    }

    /// SPI1-backed backend assuming kernel-side pre-wiring.
    pub fn new_spi1_pre_wired() -> Result<Self, BackendInitError> {
        Self::new_with_pre_wired_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi1())
    }

    /// SPI2-backed backend assuming kernel-side pre-wiring.
    pub fn new_spi2_pre_wired() -> Result<Self, BackendInitError> {
        Self::new_with_pre_wired_descriptor(Ast10x0BoardDescriptor::ast10x0_qemu_default_spi2())
    }

    pub fn info(&self, cs: ChipSelect) -> FlashInfo {
        let cfg = match self.cfg_for(cs) {
            Ok(cfg) => cfg,
            Err(_) => {
                return FlashInfo {
                    capacity: 0,
                    chunk_size: 0,
                    erase_size: 0,
                };
            }
        };
        FlashInfo {
            capacity: cfg.capacity_mb * 1024 * 1024,
            chunk_size: cfg.page_size,
            erase_size: cfg.sector_size,
        }
    }

    pub fn exists(&mut self, cs: ChipSelect) -> Result<bool, BackendError> {
        let id = self.with_flash(cs, |flash| flash.jedec_id())?;
        Ok(id != [0x00, 0x00, 0x00] && id != [0xFF, 0xFF, 0xFF])
    }

    pub fn read(
        &mut self,
        cs: ChipSelect,
        address: u32,
        out: &mut [u8],
    ) -> Result<usize, BackendError> {
        self.with_flash(cs, |flash| flash.read(address, out))
    }

    pub fn write(
        &mut self,
        cs: ChipSelect,
        address: u32,
        data: &[u8],
    ) -> Result<usize, BackendError> {
        if data.is_empty() {
            return Ok(0);
        }

        let cfg = self.cfg_for(cs)?;
        let page_size = cfg.page_size as usize;
        if (address as usize) % page_size != 0 {
            return Err(BackendError::InvalidAddress);
        }

        self.with_flash(cs, |flash| flash.program(address, data))
    }

    pub fn erase(
        &mut self,
        cs: ChipSelect,
        address: u32,
        length: u32,
    ) -> Result<(), BackendError> {
        if length == 0 {
            return Ok(());
        }

        let cfg = self.cfg_for(cs)?;
        let erase_size = cfg.sector_size;
        if !address.is_multiple_of(erase_size) || !length.is_multiple_of(erase_size) {
            return Err(BackendError::InvalidLength);
        }

        self.with_flash(cs, |flash| flash.erase_range(address, length as usize))
    }

    fn cfg_for(&self, cs: ChipSelect) -> Result<FlashConfig, BackendError> {
        match cs {
            ChipSelect::Cs0 => Ok(self.cs0_cfg),
            ChipSelect::Cs1 => self.cs1_cfg.ok_or(BackendError::InvalidOperation),
        }
    }

    fn with_flash<R>(
        &mut self,
        cs: ChipSelect,
        f: impl FnOnce(&mut SpiNorFlash<'_>) -> Result<R, SmcError>,
    ) -> Result<R, BackendError> {
        let cfg = self.cfg_for(cs)?;
        let mut flash = match &mut self.controller {
            ControllerBackend::Fmc(fmc) => SpiNorFlash::from_fmc_cs(fmc, cfg, cs),
            ControllerBackend::Spi(spi) => SpiNorFlash::from_spi_cs(spi, cfg, cs),
        }
        .map_err(smc_to_backend_error)?;
        f(&mut flash).map_err(smc_to_backend_error)
    }
}

fn should_init_normal_read(controller: SmcController, cs0_cfg: FlashConfig) -> bool {
    // QEMU FMC flash models hang on the calibration/mapped-read path used by
    // spi_nor_read_init(). The virtual FMC descriptors use 1 MiB CS geometry;
    // physical board descriptors are larger and still need read-mode setup.
    !(controller == SmcController::Fmc && cs0_cfg.capacity_mb <= 1)
}

fn build_smc_controller(
    controller: SmcController,
    cs0_cfg: FlashConfig,
    cs1_cfg: Option<FlashConfig>,
) -> Result<ControllerBackend, SmcError> {
    let init_normal_read = should_init_normal_read(controller, cs0_cfg);

    let config = SmcConfig {
        controller_id: controller,
        cs0: Some(cs0_cfg),
        cs1: cs1_cfg,
        dma_enabled: false,
        enable_interrupts: false,
        topology: SmcTopology::BootSpi { master_idx: 0 },
    };

    match controller {
        SmcController::Fmc => {
            // SAFETY: backend owns the FMC controller for the process lifetime.
            let uninit = unsafe { FmcUninit::new(config) }?;
            let mut fmc = uninit.init()?;
            if init_normal_read {
                fmc.spi_nor_read_init(ChipSelect::Cs0)?;
                if cs1_cfg.is_some() {
                    fmc.spi_nor_read_init(ChipSelect::Cs1)?;
                }
            }
            Ok(ControllerBackend::Fmc(fmc))
        }
        SmcController::Spi1 | SmcController::Spi2 => {
            // SAFETY: backend owns the SPI controller for the process lifetime.
            let uninit = unsafe { SpiUninit::new(controller, config) }?;
            let mut spi = uninit.init()?;
            spi.spi_nor_read_init(ChipSelect::Cs0)?;
            if cs1_cfg.is_some() {
                spi.spi_nor_read_init(ChipSelect::Cs1)?;
            }
            Ok(ControllerBackend::Spi(spi))
        }
    }
}

pub type Backend = Ast10x0FlashBackend;
