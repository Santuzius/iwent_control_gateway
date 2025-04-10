// src/main.rs
use std::sync::{Arc, RwLock};
use tokio::{signal, sync::{broadcast, mpsc}}; // For graceful shutdown on Ctrl+C

mod can;
mod data;
mod error;
mod modbus_server;
mod gpio;
mod modbus_client;

use data::BmsData;
use error::AppError; // Import the AppError type

// --- Define Command Enum for Broadcast Channel ---
#[derive(Debug, Clone, PartialEq, Eq)] // Ensure it can be cloned and compared
pub enum SystemCommand {
    InvertersOff,
    InvertersOn,
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    env_logger::init();

    log::info!("Application starting...");

    // Create shared data structures with thread-safe access
    let bms_data1: Arc<RwLock<Option<BmsData>>> = Arc::new(RwLock::new(Some(BmsData {
        min_cell_voltage: Some(0),
        max_cell_voltage: Some(0),
        min_temperature: Some(0),
        max_temperature: Some(0),
        info: Some(0xFF),
        soc: Some(0),
        current: Some(0),
        total_voltage: Some(0),
        warning1: Some(0),
        warning2: Some(0),
        error1: Some(0xFF),
        error2: Some(0xFF),
        on: Some(0),
        quit: Some(0),
    })));

    let bms_data2: Arc<RwLock<Option<BmsData>>> = Arc::new(RwLock::new(Some(BmsData {
        min_cell_voltage: Some(0),
        max_cell_voltage: Some(0),
        min_temperature: Some(0),
        max_temperature: Some(0),
        info: Some(0xFF),
        soc: Some(0),
        current: Some(0),
        total_voltage: Some(0),
        warning1: Some(0),
        warning2: Some(0),
        error1: Some(0xFF),
        error2: Some(0xFF),
        on: Some(0),
        quit: Some(0),
    })));

    // --- Create Communication Channels ---

    // 1. Channel for GPIO Input -> Main Task
    //    (Buffer 1 is likely sufficient as we process state changes)
    let (gpio_tx, mut main_gpio_rx) = mpsc::channel::<bool>(2);

    // 2. Broadcast Channel for Main Task -> Modbus Clients & GPIO Output
    //    (Buffer size depends on how quickly receivers process, 16 is arbitrary)
    let (main_cmd_tx, _) = broadcast::channel::<SystemCommand>(16);

    // --- Spawn asynchronous tasks ---

    log::info!("Spawning CAN and Modbus Server tasks...");
    // CAN Receiver tasks
    let can_rx1_handle = tokio::spawn(can::rx_task(
        "can0",
        1, 
        Arc::clone(&bms_data1)
    ));
    let can_rx2_handle = tokio::spawn(can::rx_task(
        "can0",
        2, 
        Arc::clone(&bms_data2)
    ));

    // CAN Transmitter task (placeholder)
    let can_tx_handle = tokio::spawn(can::tx_task(
        "can0"
    ));

    // Modbus Server tasks
    let modbus_server1_handle = tokio::spawn(modbus_server::task(
        "172.18.143.93:40502", // Address for BMS 1 server
        Arc::clone(&bms_data1),
    ));
    let modbus_server2_handle = tokio::spawn(modbus_server::task(
        "172.18.143.93:41502", // Address for BMS 2 server
        Arc::clone(&bms_data2),
    ));

    // --- Spawn new tasks ---
    log::info!("Spawning GPIO and Modbus Client tasks...");

    // GPIO Input Task
    let gp_in_handle = tokio::spawn(gpio::input_task(
        gpio_tx
    ));

    // GPIO Output Task (subscribes to broadcast channel)
    let gp_out_handle = tokio::spawn(gpio::output_task(
        main_cmd_tx.subscribe()
    ));

    // Modbus Client Tasks (each subscribes to broadcast channel)
    let modbus_client1_handle = tokio::spawn(modbus_client::task(
        "192.168.2.100:30502", // Inverter 1 Address
        main_cmd_tx.subscribe(),
    ));
    let modbus_client2_handle = tokio::spawn(modbus_client::task(
        "192.168.2.100:31502", // Inverter 2 Address
        main_cmd_tx.subscribe(),
    ));

    log::info!("All tasks spawned. Running main control loop.");

    // --- Main Control Loop ---
    // This loop waits for state changes from the GPIO input task
    // and broadcasts commands accordingly.
    loop {
        tokio::select! {
             // Wait for state updates from GPIO Input task
             Some(gpio_is_high) = main_gpio_rx.recv() => {
                 log::info!("Main: Received GPIO state update: {}", if gpio_is_high { "HIGH" } else { "LOW" });

                 // Determine command based on state (assuming pull-up: HIGH = Inactive/ON, LOW = Active/OFF)
                 // **ADJUST THIS LOGIC BASED ON YOUR ACTUAL HARDWARE WIRING (PULL-UP/PULL-DOWN) AND REQUIREMENT**
                 let command = if gpio_is_high {
                     SystemCommand::InvertersOn // GPIO HIGH -> Turn ON (or do nothing)
                 } else {
                     SystemCommand::InvertersOff // GPIO LOW -> Turn OFF
                 };

                 log::info!("Main: Broadcasting command: {:?}", command);
                 // Send command via broadcast channel
                 // Ignore error if no receivers are active (should not happen normally)
                 if let Err(e) = main_cmd_tx.send(command.clone()) {
                    // Log if *all* receivers are gone, which indicates a problem
                    // A single receiver dropping might be normal if a client task restarts
                     if main_cmd_tx.receiver_count() == 0 {
                         log::error!("Main: Failed to broadcast command, no active receivers: {}", e);
                         // Consider breaking the loop or other error handling if this state is critical
                     } else {
                          log::warn!("Main: Failed to broadcast to some receivers (count={}): {}", main_cmd_tx.receiver_count(), e);
                     }
                 }

             }
              // Handle Ctrl+C signal for graceful shutdown
              _ = signal::ctrl_c() => {
                 log::info!("Main: Ctrl+C received. Shutting down.");
                 break; // Exit the main loop
              }
              // Add other conditions or timeouts if needed
        }
    }

    // --- Graceful Shutdown ---
    log::info!("Main: Aborting all tasks...");
    // Abort all spawned tasks
    can_rx1_handle.abort();
    can_rx2_handle.abort();
    can_tx_handle.abort();
    modbus_server1_handle.abort();
    modbus_server2_handle.abort();
    gp_in_handle.abort();
    gp_out_handle.abort();
    modbus_client1_handle.abort();
    modbus_client2_handle.abort();

    log::info!("Application finished.");
    Ok(())
}
