// src/main.rs
use std::sync::{Arc, RwLock};
use tokio::signal; // For graceful shutdown on Ctrl+C

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
    Off,
    On,
    Quit
}

fn reset_control_frozen(
    bms_data1: Arc<RwLock<Option<BmsData>>>,
    bms_data2: Arc<RwLock<Option<BmsData>>>,
) -> Result<(), AppError> {
    std::thread::sleep(std::time::Duration::from_secs(1));

    {
        let mut data_guard1 = bms_data1.write().map_err(|_| AppError::LockPoisoned)?;
        let data = data_guard1.get_or_insert_default();
        data.control_frozen = Some(false);
    }

    {
        let mut data_guard2 = bms_data2.write().map_err(|_| AppError::LockPoisoned)?;
        let data = data_guard2.get_or_insert_default();
        data.control_frozen = Some(false);
    }

    log::debug!("Control frozen reset after 1 second.");
    Ok(())
}

async fn input_flag_manager_task(
    bms_data1: Arc<RwLock<Option<BmsData>>>,
    bms_data2: Arc<RwLock<Option<BmsData>>>,
    input_rx: std::sync::mpsc::Receiver<SystemCommand>,
    output_tx: crossbeam_channel::Sender<SystemCommand>
)  -> Result<(), AppError> {

    for msg in input_rx.iter() {
        let control_frozen1;
        {
            let data_guard1 = bms_data1.read().map_err(|_| {
                log::error!("Failed to acquire read lock 1 (poisoned)");
                AppError::LockPoisoned
            })?;
    
            let maybe_data1 = &*data_guard1;

            match maybe_data1 {
                Some(data) => {
                    control_frozen1 = data.control_frozen.unwrap();
                }
                None => {
                    control_frozen1 = false;
                    log::warn!("No BmsData1 object available yet.");
                }
            }
        }

        let control_frozen2;
        {
            let data_guard2 = bms_data2.read().map_err(|_| {
                log::error!("Failed to acquire read lock 2 (poisoned)");
                AppError::LockPoisoned
            })?;
    
            let maybe_data2 = &*data_guard2;

            match maybe_data2 {
                Some(data) => {
                    control_frozen2 = data.control_frozen.unwrap();
                }
                None => {
                    control_frozen2 = false;
                    log::warn!("No BmsData2 object available yet.");
                }
            }
        }

        let control_frozen = control_frozen1 || control_frozen2;
        if !control_frozen {
            {
                let mut data_guard1 = bms_data1.write().map_err(|_| AppError::LockPoisoned)?;
                let data_ref1 = data_guard1.get_or_insert_default();
                data_ref1.control_frozen = Some(true);
                log::debug!("Control for BMS 1 frozen.");
            }

            {
                let mut data_guard2 = bms_data2.write().map_err(|_| AppError::LockPoisoned)?;
                let data_ref2 = data_guard2.get_or_insert_default();
                data_ref2.control_frozen = Some(true);
                log::debug!("Control for BMS 2 frozen.");
            }

            let bms_data1_clone = Arc::clone(&bms_data1);
            let bms_data2_clone = Arc::clone(&bms_data2);
            std::thread::spawn(move || reset_control_frozen(bms_data1_clone, bms_data2_clone));
            if let Err(e) = output_tx.send(msg.clone()) {
                log::error!(
                    "Error when sending {:#?}: {:?}",
                    msg,
                    e
                );
            } else {
                log::debug!("{:#?} sent.", msg);
            }
        }
    }

    Ok(())
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
        control_frozen: Some(false),
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
        control_frozen: Some(false),
    })));

    // --- Create Communication Channels ---

    // 1. Channel for system commands from input
    let (input_tx1, input_rx) = std::sync::mpsc::channel::<SystemCommand>();
    let input_tx2 = input_tx1.clone();
    let input_tx3 = input_tx2.clone();

    // 1. Channel for errors from CAN
    let (error_tx1, error_rx1) = crossbeam_channel::unbounded::<()>();
    let error_tx2 = error_tx1.clone();
    let error_rx2 = error_rx1.clone();
    let error_rx3 = error_rx2.clone();

    // 2. Broadcast Channel for system commands to output
    let (output_tx, output_rx1) = crossbeam_channel::unbounded::<SystemCommand>();
    let output_rx2 = output_rx1.clone();
    let output_rx3 = output_rx2.clone();
    let output_rx4 = output_rx3.clone();

    // --- Spawn asynchronous tasks ---
    log::info!("Spawning input tasks...");

    // CAN Receiver tasks
    let can_rx1_handle = tokio::spawn(can::rx_task(
        "can0",
        1, 
        Arc::clone(&bms_data1),
        error_tx1,
    ));
    let can_rx2_handle = tokio::spawn(can::rx_task(
        "can0",
        2, 
        Arc::clone(&bms_data2),
        error_tx2,
    ));

    // GPIO Input Task
    let gp_in_handle = tokio::spawn(gpio::input_task(
        input_tx1
    ));

    // Modbus Server tasks
    let modbus_server1_handle = tokio::spawn(modbus_server::task(
        "172.18.143.93:40502", // Address for BMS 1 server
        Arc::clone(&bms_data1),
        input_tx2
    ));
    let modbus_server2_handle = tokio::spawn(modbus_server::task(
        "172.18.143.93:41502", // Address for BMS 2 server
        Arc::clone(&bms_data2),
        input_tx3
    ));

    log::info!("Spawning output tasks...");

    // Modbus Client Tasks (each subscribes to broadcast channel)
    let modbus_client1_handle = tokio::spawn(modbus_client::task(
        "192.168.2.100:30502", // Inverter 1 Address
        error_rx1,
        output_rx1
    ));
    let modbus_client2_handle = tokio::spawn(modbus_client::task(
        "192.168.2.100:31502", // Inverter 2 Address
        error_rx2,
        output_rx2
    ));

    // CAN Transmitter task
    let can_tx_handle = tokio::spawn(can::tx_task(
        "can0",
        output_rx3
    ));

    // GPIO Output Task (subscribes to broadcast channel)
    let gp_out_handle = tokio::spawn(gpio::output_task(
        error_rx3,
        output_rx4
    ));

    log::info!("Spawning input flag manager task...");

    let input_flag_manager_handle = tokio::spawn(input_flag_manager_task(
        Arc::clone(&bms_data1),
        Arc::clone(&bms_data2),
        input_rx,
        output_tx
    ));

    log::info!("All tasks spawned.");

    // --- Main Control Loop ---
    // This loop waits for state changes from the GPIO input task
    // and broadcasts commands accordingly.
    loop {
        tokio::select! {
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
    gp_in_handle.abort();
    modbus_server1_handle.abort();
    modbus_server2_handle.abort();
    modbus_client1_handle.abort();
    modbus_client2_handle.abort();
    can_tx_handle.abort();
    gp_out_handle.abort();
    input_flag_manager_handle.abort();

    log::info!("Application finished.");
    Ok(())
}
