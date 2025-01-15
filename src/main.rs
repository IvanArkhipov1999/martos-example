#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::uart::Uart;
use esp_hal::Blocking;
use esp_hal::{entry, time};
use esp_println::println;
use esp_wifi::esp_now::{EspNow, PeerInfo, BROADCAST_ADDRESS};
use martos::{get_esp_now, get_io, get_uart2};
use martos::{
    init_system,
    task_manager::{TaskManager, TaskManagerTrait},
};

/// Esp-now object for network
static mut ESP_NOW: Option<EspNow> = None;
/// Variable for saving time to send broadcast message
static mut NEXT_SEND_TIME: Option<u64> = None;

// Uart for reading bytes
static mut UART2: Option<Uart<'_, esp_hal::peripherals::UART2, Blocking>> = None;

/// Setup function for task to execute.
fn setup_fn() {
    println!("Setup hello world!");

    unsafe {
        let uart2 = get_uart2();
        let io = get_io();
        let uart = Uart::new(uart2, io.pins.gpio1, io.pins.gpio2).unwrap();
        UART2 = Some(uart);

        ESP_NOW = Some(get_esp_now());
        NEXT_SEND_TIME = Some(time::now().duration_since_epoch().to_millis() + 5 * 1000);
    }
}

/// Loop function for task to execute.
fn loop_fn() {
    unsafe {
        // // Sending broadcast messages and receiving them
        // let mut esp_now = ESP_NOW.take().expect("Esp-now error in main");

        // let r = esp_now.receive();
        // if let Some(r) = r {
        //     println!("Received {:?}", r);

        //     if r.info.dst_address == BROADCAST_ADDRESS {
        //         if !esp_now.peer_exists(&r.info.src_address) {
        //             esp_now
        //                 .add_peer(PeerInfo {
        //                     peer_address: r.info.src_address,
        //                     lmk: None,
        //                     channel: None,
        //                     encrypt: false,
        //                 })
        //                 .unwrap();
        //         }
        //         let status = esp_now
        //             .send(&r.info.src_address, b"Hello Peer")
        //             .unwrap()
        //             .wait();
        //         println!("Send hello to peer status: {:?}", status);
        //     }
        // }

        // let mut next_send_time = NEXT_SEND_TIME.take().expect("Next send time error in main");
        // if time::now().duration_since_epoch().to_millis() >= next_send_time {
        //     next_send_time = time::now().duration_since_epoch().to_millis() + 5 * 1000;
        //     println!("Send");
        //     let status = esp_now
        //         .send(&BROADCAST_ADDRESS, b"0123456789")
        //         .unwrap()
        //         .wait();
        //     println!("Send broadcast status: {:?}", status)
        // }

        // NEXT_SEND_TIME = Some(next_send_time);

        // Init buf
        const BUFFER_SIZE: usize = 24;
        let mut buf = [0u8; 2 * BUFFER_SIZE];
        let buf2 = &mut [0u8; BUFFER_SIZE];
        let i2: usize = buf2.len();

        // Reading bytes from uart2 to buf2
        let mut uart2 = UART2.take().expect("Uart2 error in main");
        let _ = uart2.read_bytes(buf2);
        println!("Read bytes: {:?}", buf2);
        UART2 = Some(uart2);

        // bytesToHex
        let hex = "0123456789ABCDEF";
        for i in 0..i2 {
            let b = buf2[i];
            buf[2 * i] = hex.as_bytes()[(b >> 4) as usize]; // Получаем старшую часть байта
            buf[2 * i + 1] = hex.as_bytes()[(b & 0x0F) as usize]; // Получаем младшую часть байта
        }
        println!("bytesToHex result: {:?}", buf);
        
        // Send buf to broadcast
        println!("Send");
        let mut esp_now = ESP_NOW.take().expect("Esp-now error in main");
        let status = esp_now
            .send(&BROADCAST_ADDRESS, &buf)
            .unwrap()
            .wait();
        println!("Send broadcast status: {:?}", status);

        ESP_NOW = Some(esp_now);
    }
}

/// Stop condition function for task to execute.
fn stop_condition_fn() -> bool {
    return false;
}

#[entry]
fn main() -> ! {
    // Initialize Martos.
    init_system();
    // Add task to execute.
    TaskManager::add_task(setup_fn, loop_fn, stop_condition_fn);
    // Start task manager.
    TaskManager::start_task_manager();
}
