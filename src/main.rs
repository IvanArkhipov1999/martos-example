#![no_std]
#![no_main]

use core::ptr::addr_of_mut;
use esp_backtrace as _;
use esp_hal::entry;
use esp_hal::uart::{config::Config, Uart};
use esp_hal::Blocking;
use esp_println::println;
use esp_wifi::esp_now::{EspNow, ReceiveInfo, BROADCAST_ADDRESS};
use martos::{get_esp_now, get_io, get_uart2};
use martos::{
    init_system,
    task_manager::{TaskManager, TaskManagerTrait},
};

/// Esp-now object for network
static mut ESP_NOW: Option<EspNow> = None;
/// Uart2 для чтения/записи байтов
static mut UART2: Option<Uart<'_, esp_hal::peripherals::UART2, Blocking>> = None;
/// Буферы как в оригинальной программе
static mut BUF: [u8; 1024] = [0; 1024];
static mut BUF2: [u8; 1024] = [0; 1024];
static mut I2: usize = 0;

/// Setup function for task to execute.
fn setup_fn() {
    println!("ESP-NOW UART Bridge Setup");

    unsafe {
        let uart2 = get_uart2();
        let io = get_io();

        // Правильные настройки как в оригинальной программе
        // UART1: RX=16, TX=17, 19200 baud, 8N1
        let config = Config::default()
            .baudrate(19200)
            .data_bits(esp_hal::uart::config::DataBits::DataBits8)
            .parity_none() // Используем parity_none() вместо parity()
            .stop_bits(esp_hal::uart::config::StopBits::STOP1);

        let uart = Uart::new_with_config(
            uart2,
            config,
            io.pins.gpio16, // RX pin
            io.pins.gpio17, // TX pin
        )
        .expect("UART init failed");

        UART2 = Some(uart);

        // Инициализируем ESP-NOW
        ESP_NOW = Some(get_esp_now());

        println!("ESP-NOW UART Bridge initialized");
        println!("UART2: 19200 8N1, RX=16, TX=17");
    }
}

/// Конвертация bytes в hex (bytesToHex из оригинала)
fn bytes_to_hex(input: &[u8], output: &mut [u8]) {
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";

    for (i, &byte) in input.iter().enumerate() {
        if 2 * i + 1 < output.len() {
            output[2 * i] = HEX_CHARS[(byte >> 4) as usize];
            output[2 * i + 1] = HEX_CHARS[(byte & 0x0F) as usize];
        }
    }
}

/// Конвертация hex в bytes (из receiveCallback оригинала)
fn hex_to_bytes(hex_data: &[u8], output: &mut [u8]) -> usize {
    let mut bytes_written = 0;

    for i in (0..hex_data.len()).step_by(2) {
        if i + 1 < hex_data.len() && bytes_written < output.len() {
            // Берем 2 символа hex
            let hex_byte = [hex_data[i], hex_data[i + 1]];

            // Конвертируем в число
            let high = char_to_hex_digit(hex_byte[0] as char);
            let low = char_to_hex_digit(hex_byte[1] as char);

            if let (Some(h), Some(l)) = (high, low) {
                output[bytes_written] = (h << 4) | l;
                bytes_written += 1;
            }
        }
    }

    bytes_written
}

/// Вспомогательная функция для конверсии символа в hex цифру
fn char_to_hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some((c as u8) - b'0'),
        'A'..='F' => Some((c as u8) - b'A' + 10),
        'a'..='f' => Some((c as u8) - b'a' + 10),
        _ => None,
    }
}

/// Обработка полученных ESP-NOW сообщений (receiveCallback из оригинала)
fn handle_esp_now_receive(receive_info: &ReceiveInfo, data: &[u8]) {
    println!(
        "Received ESP-NOW message from: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        receive_info.src_address[0],
        receive_info.src_address[1],
        receive_info.src_address[2],
        receive_info.src_address[3],
        receive_info.src_address[4],
        receive_info.src_address[5]
    );

    unsafe {
        // Ограничиваем длину как в оригинале (ESP_NOW_MAX_DATA_LEN = 250)
        let msg_len = core::cmp::min(250, data.len());
        let received_data = &data[..msg_len];

        // Конвертируем hex в bytes как в оригинале
        let mut buffer2 = [0u8; 125]; // msg_len/2
        let bytes_written = hex_to_bytes(received_data, &mut buffer2);

        // Отправляем в UART как Serial2.write() в оригинале
        if let Some(ref mut uart) = UART2 {
            if let Err(e) = uart.write_bytes(&buffer2[..bytes_written]) {
                println!("UART write error: {:?}", e);
            } else {
                println!("Sent {} bytes to UART", bytes_written);
            }
        }
    }
}

/// Loop function for task to execute.
fn loop_fn() {
    unsafe {
        // 1. Обработка входящих ESP-NOW сообщений
        if let Some(ref mut esp_now) = ESP_NOW {
            if let Some(received) = esp_now.receive() {
                handle_esp_now_receive(&received.info, &received.data);
            }
        }

        // 2. Чтение данных из UART (как в loop() оригинала)
        if let Some(ref mut uart) = UART2 {
            // Проверяем доступность данных и читаем по байтам
            let mut temp_buf = [0u8; 1];

            while uart.read_bytes(&mut temp_buf).is_ok() {
                let i2_ptr = addr_of_mut!(I2);
                let buf2_ptr = addr_of_mut!(BUF2);

                (*buf2_ptr)[*i2_ptr] = temp_buf[0];
                *i2_ptr += 1;

                // Если буфер заполнен или достигли лимита
                if *i2_ptr >= (*buf2_ptr).len() {
                    // Обрабатываем накопленные данные
                    process_uart_buffer();
                    *i2_ptr = 0; // Сбрасываем индекс
                    break;
                }
            }

            // Также обрабатываем данные если накопилось достаточно
            let i2_val = *addr_of_mut!(I2);
            if i2_val > 0 && i2_val % 32 == 0 {
                // Обрабатываем каждые 32 байта
                process_uart_buffer();
                *addr_of_mut!(I2) = 0;
            }
        }
    }
}

/// Обработка буфера UART данных
fn process_uart_buffer() {
    unsafe {
        let i2_val = *addr_of_mut!(I2);

        if i2_val > 0 {
            println!("Processing {} bytes from UART", i2_val);

            // Конвертируем bytes в hex (bytesToHex из оригинала)
            let buf2_ptr = addr_of_mut!(BUF2);
            let buf_ptr = addr_of_mut!(BUF);

            bytes_to_hex(&(*buf2_ptr)[..i2_val], &mut (*buf_ptr));

            // Отправляем через ESP-NOW (broadcast из оригинала)
            if let Some(ref mut esp_now) = ESP_NOW {
                let hex_data = &(*buf_ptr)[..i2_val * 2]; // каждый байт становится 2 hex символами

                match esp_now.send(&BROADCAST_ADDRESS, hex_data) {
                    Ok(send_result) => {
                        let status = send_result.wait();
                        println!("ESP-NOW broadcast status: {:?}", status);
                    }
                    Err(e) => {
                        println!("ESP-NOW send error: {:?}", e);
                    }
                }
            }
        }
    }
}

/// Stop condition function for task to execute.
fn stop_condition_fn() -> bool {
    false // Никогда не останавливаем задачу
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
