// Licensed under the Apache-2.0 license
// SPDX-License-Identifier: Apache-2.0

#![no_main]
#![no_std]

use app_flash_client::handle;
use flash_client::FlashClient;
use userspace::entry;
use userspace::syscall;

const PAGE_SIZE: usize = 256;
// Last 4 KiB sector of the 1 MiB FMC flash. The QEMU m25p80 model is
// blank at boot (the kernel image is loaded into CPU address space, not
// written to the SPI flash backing) so any sector is safe; we use the
// last one as a convention that maps to "off the end of the image" on
// real silicon.
const FMC_TEST_OFFSET: u32 = 0xFF000;

fn check_capacity(
    controller_id: u32,
    client: &FlashClient,
    expected: u32,
) -> Result<(), pw_status::Error> {
    let capacity = client.capacity().map_err(|_| pw_status::Error::Internal)?;
    if capacity != expected {
        pw_log::error!(
            "flash capacity mismatch controller={} got={} expected={}",
            controller_id as u32,
            capacity as u32,
            expected as u32
        );
        return Err(pw_status::Error::Unknown);
    }
    pw_log::info!(
        "flash capacity ok controller={} bytes={}",
        controller_id as u32,
        capacity as u32
    );
    Ok(())
}

fn check_fmc_mapped_read(client: &FlashClient) -> Result<(), pw_status::Error> {
    let mut readback = [0u8; PAGE_SIZE];
    let n = client
        .read(FMC_TEST_OFFSET, &mut readback)
        .map_err(|_| pw_status::Error::Internal)?;
    if n != PAGE_SIZE {
        pw_log::error!("fmc mapped read returned short count (n={})", n as u32);
        return Err(pw_status::Error::Unknown);
    }
    pw_log::info!(
        "fmc mapped read passed offset=0x{:x}",
        FMC_TEST_OFFSET as u32
    );
    Ok(())
}

#[entry]
fn entry() {
    let fmc = FlashClient::new(handle::FLASH_FMC);
    let spi1 = FlashClient::new(handle::FLASH_SPI1);
    let spi2 = FlashClient::new(handle::FLASH_SPI2);

    let status = check_capacity(0, &fmc, 1024 * 1024)
        .and_then(|_| check_capacity(1, &spi1, 32 * 1024 * 1024))
        .and_then(|_| check_capacity(2, &spi2, 32 * 1024 * 1024))
        .and_then(|_| check_fmc_mapped_read(&fmc));

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
