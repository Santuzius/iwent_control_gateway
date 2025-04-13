// src/gpio.rs

use crate::SystemCommand; // Import the command enum from main or a shared module
use crate::error::AppError;
use std::time::Duration;
use rppal::gpio::Gpio;
use tokio::time::sleep;

// --- GPIO Pin Definitions ---
const PIN_OFF: u8 = 13;
const PIN_ON: u8 = 6;
const PIN_QUIT: u8 = 16;
const PIN_RED_LED: u8 = 22;
const PIN_GREEN_LED: u8 = 23;

// Debounce time for inputs
const DEBOUNCE_DURATION: Duration = Duration::from_millis(25);
// Poll interval to check button state - adjust as needed
const POLL_INTERVAL: Duration = Duration::from_millis(50);

// --- GPIO Input Task (unverändert) ---
/// Monitors GPIO input pins for On, Off, and Quit signals and sends corresponding SystemCommands.
pub async fn input_task(input_tx: std::sync::mpsc::Sender<SystemCommand>) -> Result<(), AppError> {
    {
        log::info!("Initializing GPIO input task for Raspberry Pi...");
        // Initialize GPIO
        let gpio = Gpio::new().map_err(AppError::Gpio)?;

        // Configure input pins with pull-down resistors
        // Rppal doesn't have built-in debounce config, so we handle it manually after reading.
        let pin_off = gpio.get(PIN_OFF)
            .map_err( AppError::Gpio)?
            .into_input_pulldown();
        let pin_on = gpio.get(PIN_ON)
            .map_err( AppError::Gpio)?
            .into_input_pulldown();
        let pin_quit = gpio.get(PIN_QUIT)
            .map_err( AppError::Gpio)?
            .into_input_pulldown();

        log::info!("GPIO inputs initialized (Off: {}, On: {}, Quit: {}). Starting poll loop.", PIN_OFF, PIN_ON, PIN_QUIT);

        // State tracking to detect changes
        let mut last_off_state = false;
        let mut last_on_state = false;
        let mut last_quit_state = false;

        loop {
            let current_off_state = pin_off.is_high();
            let current_on_state = pin_on.is_high();
            let current_quit_state = pin_quit.is_high();

            // --- Off Button Logic ---
            if current_off_state && !last_off_state {
                // Rising edge detected
                sleep(DEBOUNCE_DURATION).await; // Wait for debounce
                if pin_off.is_high() { // Re-check state after debounce
                    log::debug!("Off button pressed (Pin {})", PIN_OFF);
                    // Send command only once per press
                    input_tx.send(SystemCommand::Off).map_err(|e| AppError::SendError(format!("Failed to send Off command: {}", e)))?;
                    last_off_state = true; // Mark as pressed
                }
            } else if !current_off_state && last_off_state {
                // Falling edge detected (button released)
                 log::debug!("Off button released (Pin {})", PIN_OFF);
                last_off_state = false; // Mark as released
            }

            // --- On Button Logic ---
            if current_on_state && !last_on_state {
                sleep(DEBOUNCE_DURATION).await;
                if pin_on.is_high() {
                    log::debug!("On button pressed (Pin {})", PIN_ON);
                    input_tx.send(SystemCommand::On).map_err(|e| AppError::SendError(format!("Failed to send On command: {}", e)))?;
                    last_on_state = true;
                }
            } else if !current_on_state && last_on_state {
                 log::debug!("On button released (Pin {})", PIN_ON);
                last_on_state = false;
            }

            // --- Quit Button Logic ---
            if current_quit_state && !last_quit_state {
                sleep(DEBOUNCE_DURATION).await;
                if pin_quit.is_high() {
                    log::debug!("Quit button pressed (Pin {})", PIN_QUIT);
                    input_tx.send(SystemCommand::Quit).map_err(|e| AppError::SendError(format!("Failed to send Quit command: {}", e)))?;
                    last_quit_state = true;
                }
            } else if !current_quit_state && last_quit_state {
                 log::debug!("Quit button released (Pin {})", PIN_QUIT);
                last_quit_state = false;
            }

            // Prevent busy-waiting
            sleep(POLL_INTERVAL).await;
        }
        // Note: The loop runs indefinitely. The Quit command signals other parts
        // of the application via the channel, but doesn't stop this task directly.
        // Consider adding a mechanism to gracefully shutdown this task if needed.
    }
}

// --- GPIO Output Task ---
/// Controls LEDs based on commands received from `output_rx` and error signals from `error_rx`.
pub async fn output_task(
    error_rx: crossbeam_channel::Receiver<()>, // Original crossbeam receiver
    output_rx: crossbeam_channel::Receiver<SystemCommand>, // Original crossbeam receiver
) -> Result<(), AppError> {

    // --- Main Logic (using the bridge receivers) ---
    {
        log::info!("Initializing GPIO output task for Raspberry Pi...");
        let gpio = Gpio::new().map_err(AppError::Gpio)?;

        // Configure output pins, initial level low (off)
        let mut red_led = gpio.get(PIN_RED_LED)
            .map_err(AppError::Gpio)?
            .into_output_low(); // Initializes low
        let mut green_led = gpio.get(PIN_GREEN_LED)
            .map_err(AppError::Gpio)?
            .into_output_low(); // Initializes low

        log::info!("GPIO outputs initialized (Red: {}, Green: {}). Starting event loop.", PIN_RED_LED, PIN_GREEN_LED);

        loop {
            crossbeam_channel::select! {
                recv(error_rx) -> err_msg => {
                    match err_msg {
                        Ok(_) => {
                            log::error!("Error signal received. Setting LEDs ON.");
                            red_led.set_high();
                            green_led.set_high();
                        },
                        Err(_) => {
                            log::warn!("Error channel closed. Handling closure accordingly.");
                            // Eventuell beenden oder eine alternative Logik verwenden
                        }
                    }
                },
                recv(output_rx) -> cmd_msg => {
                    match cmd_msg {
                        Ok(command) => {
                            log::debug!("Received command: {:?}", command);
                            match command {
                                SystemCommand::On => {
                                    log::info!("Setting Green LED ON, Red LED OFF.");
                                    red_led.set_low();
                                    green_led.set_high();
                                },
                                SystemCommand::Off => {
                                    log::info!("Setting Red LED ON, Green LED OFF.");
                                    red_led.set_high();
                                    green_led.set_low();
                                }
                                _ => {}
                            }
                        },
                        Err(_) => {
                            log::error!("Output channel closed. Exiting loop.");
                        }
                    }
                }
            }
        }

        // If the loop breaks (e.g., by uncommenting 'break' under Quit command)
        // log::info!("GPIO output task finished.");
        // Ok(())

    }
}
