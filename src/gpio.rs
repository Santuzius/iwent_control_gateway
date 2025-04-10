// src/gpio.rs
#![allow(unused_imports, dead_code)] // Allow unused imports/code if compiling on non-Pi

use crate::error::AppError;
use crate::SystemCommand; // Import the command enum from main or a shared module
use rppal::gpio::{Gpio, InputPin, Level, OutputPin, Trigger};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;

// --- Configuration ---
const INPUT_PIN_BCM: u8 = 13; // BCM GPIO 13 is Pin 33
const OUTPUT_PIN_BCM: u8 = 22; // BCM GPIO 22 is Pin 15
const DEBOUNCE_MS: u64 = 25;

// --- GPIO Input Task (Reads Pin 13) ---
pub async fn input_task(tx: mpsc::Sender<bool>) -> Result<(), AppError> {
    #[cfg(any(target_arch = "arm", target_arch="aarch64"))]
    {
        log::info!("Starting GPIO input task on BCM Pin {}", INPUT_PIN_BCM);
        let gpio = Gpio::new()?;
        let mut pin = gpio.get(INPUT_PIN_BCM)?.into_input_pullup(); // Changed to pullup as requested for detecting a LOW signal
        pin.set_interrupt(Trigger::Both)?; // Trigger on rising and falling edges for state changes

        // Store last known stable state
        let mut last_state = pin.read() == Level::High; // Read initial state

        // Send initial state
        if let Err(e) = tx.send(last_state).await {
             log::error!("GPIO Input: Failed to send initial state: {}", e);
             return Err(AppError::from(e)); // Return error if channel closed
        }

        loop {
            // Wait for an interrupt asynchronously
             match pin.poll_interrupt(true, None) { // poll_interrupt waits for an edge
                 Ok(Some(level)) => {
                     // Edge detected, now debounce
                     sleep(Duration::from_millis(DEBOUNCE_MS)).await;
                     let current_level_after_debounce = pin.read();

                     if current_level_after_debounce == level {
                         let new_state = level == Level::High;
                         if new_state != last_state {
                              log::info!("GPIO Input (Pin {}): State changed to {}", INPUT_PIN_BCM, if new_state { "HIGH" } else { "LOW" });
                             last_state = new_state;
                             if let Err(e) = tx.send(last_state).await {
                                 log::error!("GPIO Input: Failed to send state update: {}", e);
                                 // Decide whether to break or continue
                                 break; // Exit if main task channel closed
                             }
                         } else {
                            // log::trace!("GPIO Input (Pin {}): Debounce ignored spurious edge.", INPUT_PIN_BCM);
                         }
                     } else {
                       // log::trace!("GPIO Input (Pin {}): State changed during debounce period.", INPUT_PIN_BCM);
                     }
                 }
                 Ok(None) => {
                     // Timeout occurred (if timeout was set in poll_interrupt)
                    // log::trace!("GPIO Input (Pin {}): Poll interrupt timed out.", INPUT_PIN_BCM);
                 }
                 Err(rppal::gpio::Error::Interrupted) => {
                     // Interrupted by signal, potentially continue
                     log::warn!("GPIO Input (Pin {}): Poll interrupt interrupted.", INPUT_PIN_BCM);
                     continue;
                 }
                 Err(e) => {
                     log::error!("GPIO Input (Pin {}): Error polling interrupt: {}", INPUT_PIN_BCM, e);
                     return Err(AppError::Gpio(e));
                 }
             }
             // Small delay to prevent high CPU usage in case of continuous interrupts or errors
              sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }
    #[cfg(not(any(target_arch = "arm", target_arch="aarch64")))]
    {
        // --- Placeholder for non-Raspberry Pi builds ---
        log::warn!("GPIO input task running in placeholder mode (not on RPi).");
        // Simulate state changes for testing if needed
        let mut current_state = false;
        loop {
            sleep(Duration::from_secs(15)).await; // Simulate change every 15s
            current_state = !current_state;
            log::info!("GPIO Input (Placeholder): Simulating state change to {}", if current_state { "HIGH" } else { "LOW" });
            if tx.send(current_state).await.is_err() {
                log::error!("GPIO Input (Placeholder): Channel closed.");
                break;
            }
        }
        Ok(())
       // Err(AppError::GpioUnavailable) // Or just run placeholder loop
    }
}


// --- GPIO Output Task (Controls Pin 22 - LED) ---
pub async fn output_task(mut rx: broadcast::Receiver<SystemCommand>) -> Result<(), AppError> {
    #[cfg(any(target_arch = "arm", target_arch="aarch64"))]
    {
        log::info!("Starting GPIO output task on BCM Pin {}", OUTPUT_PIN_BCM);
        let gpio = Gpio::new()?;
        let mut pin = gpio.get(OUTPUT_PIN_BCM)?.into_output();

        // Set initial state
        pin.set_low();
        log::info!("GPIO Output (Pin {}): Initialized to LOW.", OUTPUT_PIN_BCM);

        loop {
             match rx.recv().await {
                 Ok(command) => {
                     match command {
                         SystemCommand::InvertersOff => {
                             log::info!("GPIO Output (Pin {}): Setting HIGH (Inverters Off)", OUTPUT_PIN_BCM);
                             pin.set_high();
                         }
                         SystemCommand::InvertersOn => {
                              log::info!("GPIO Output (Pin {}): Setting LOW (Inverters On)", OUTPUT_PIN_BCM);
                             pin.set_low();
                         }
                     }
                 }
                 Err(broadcast::error::RecvError::Closed) => {
                     log::warn!("GPIO Output: Command channel closed. Exiting task.");
                     break;
                 }
                 Err(broadcast::error::RecvError::Lagged(n)) => {
                      log::warn!("GPIO Output: Lagged behind on command channel by {} messages.", n);
                     // Continue receiving, skipping lagged messages
                 }
             }
        }
         Ok(())

    }
     #[cfg(not(any(target_arch = "arm", target_arch="aarch64")))]
    {
        // --- Placeholder for non-Raspberry Pi builds ---
         log::warn!("GPIO output task running in placeholder mode (not on RPi).");
         loop {
             match rx.recv().await {
                  Ok(command) => {
                     log::info!("GPIO Output (Placeholder): Received command: {:?}", command);
                     match command {
                          SystemCommand::InvertersOff => log::info!("GPIO Output (Placeholder): Would set Pin {} HIGH", OUTPUT_PIN_BCM),
                          SystemCommand::InvertersOn => log::info!("GPIO Output (Placeholder): Would set Pin {} LOW", OUTPUT_PIN_BCM),
                     }
                  }
                  Err(broadcast::error::RecvError::Closed) => {
                     log::warn!("GPIO Output (Placeholder): Command channel closed.");
                     break;
                  }
                  Err(broadcast::error::RecvError::Lagged(n)) => {
                       log::warn!("GPIO Output (Placeholder): Lagged behind on command channel by {} messages.", n);
                  }
             }
         }
         Ok(())
         //Err(AppError::GpioUnavailable)
    }
}