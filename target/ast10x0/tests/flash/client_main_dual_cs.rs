// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

#![no_main]
#![no_std]

use app_flash_client_dual_cs::handle;
use flash_client::{ClientError, FlashClient};
use userspace::entry;
use userspace::syscall;

const PAGE_SIZE: usize = 256;
const FMC_TEST_OFFSET: u32 = 0xFF000;
const CS_CAPACITY: u32 = 1024 * 1024;

fn check_per_cs_capacity(cs0: &FlashClient, cs1: &FlashClient) -> Result<(), pw_status::Error> {
    let cs0_cap = cs0.capacity().map_err(|_| pw_status::Error::Internal)?;
    let cs1_cap = cs1.capacity().map_err(|_| pw_status::Error::Internal)?;
    if cs0_cap != CS_CAPACITY || cs1_cap != CS_CAPACITY {
        pw_log::error!(
            "dual_cs capacity mismatch cs0={} cs1={} (expected {})",
            cs0_cap as u32,
            cs1_cap as u32,
            CS_CAPACITY as u32
        );
        return Err(pw_status::Error::Unknown);
    }
    pw_log::info!(
        "dual_cs per-cs capacity ok cs0={} cs1={}",
        cs0_cap as u32,
        cs1_cap as u32
    );
    Ok(())
}

fn check_cs0_mapped_read(cs0: &FlashClient) -> Result<[u8; PAGE_SIZE], pw_status::Error> {
    let mut readback = [0u8; PAGE_SIZE];
    let n = cs0
        .read(FMC_TEST_OFFSET, &mut readback)
        .map_err(|_| pw_status::Error::Internal)?;
    if n != PAGE_SIZE {
        pw_log::error!("dual_cs cs0 read short count (n={})", n as u32);
        return Err(pw_status::Error::Unknown);
    }

    pw_log::info!("dual_cs cs0 mapped read passed");
    Ok(readback)
}

fn check_cs1_not_aliasing_cs0(
    cs1: &FlashClient,
    cs0_readback: &[u8; PAGE_SIZE],
) -> Result<(), pw_status::Error> {
    // Avoid command-mode JEDEC/program transactions on QEMU's unconnected CS1:
    // they can leave the emulated SMC transfer stuck. A routed CS1 read must
    // either fail as an absent/unmapped device or return data different from
    // the CS0 mapped read.
    let mut readback = [0u8; PAGE_SIZE];
    match cs1.read(FMC_TEST_OFFSET, &mut readback) {
        Err(ClientError::ServerError(_)) | Err(ClientError::IpcError(_)) => {
            pw_log::info!("dual_cs cs1 read failed as expected on qemu");
            Ok(())
        }
        Err(_) => Err(pw_status::Error::Internal),
        Ok(n) if n == PAGE_SIZE && readback.iter().all(|&b| b == 0xFF) => {
            pw_log::info!("dual_cs cs1 read as erased/absent on qemu");
            Ok(())
        }
        Ok(n) if n == PAGE_SIZE && readback != *cs0_readback => {
            pw_log::info!("dual_cs cs1 read differs from cs0 traffic (no leak)");
            Ok(())
        }
        Ok(n) => {
            pw_log::error!("dual_cs cs1 appears aliased to cs0 (n={})", n as u32);
            Err(pw_status::Error::Unknown)
        }
    }
}

#[entry]
fn entry() {
    let cs0 = FlashClient::new(handle::FLASH_CS0);
    let cs1 = FlashClient::new(handle::FLASH_CS1);

    // Command-mode traffic can wedge QEMU's unconnected CS1, so keep this
    // dual-CS routing test to memory-mapped reads.
    let status = check_per_cs_capacity(&cs0, &cs1).and_then(|_| {
        let cs0_readback = check_cs0_mapped_read(&cs0)?;
        check_cs1_not_aliasing_cs0(&cs1, &cs0_readback)
    });

    let _ = match status {
        Ok(()) => syscall::debug_shutdown(Ok(())),
        Err(e) => syscall::debug_shutdown(Err(e)),
    };

    loop {}
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
