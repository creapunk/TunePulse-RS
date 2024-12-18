#![no_std]

pub mod inputs_dump;
use inputs_dump::InputsDump;

pub mod math_integer;
pub mod motor_driver;

pub mod analog;

use defmt_rtt as _; // Use the defmt_rtt crate for logging via RTT (Real-Time Transfer)

use motor_driver::calibration::angle_calibrator::AngleCalibrator;
use motor_driver::pwm_control::{MotorPWM, MotorType, PhasePattern};

use crate::math_integer::filters::lpf::FilterLPF;
use crate::math_integer::motion::position_integrator::Position;

use analog::supply_voltage::SupplyVoltage;

/// Represents the motor's overall calibration status.
enum MotorStatus {
    /// Motor is currently undergoing calibration.
    Calibrating,
    /// Motor calibration completed successfully and ready for normal operation.
    Ready,
    /// An error occurred during calibration or normal operation.
    Error,
}


/// The main driver struct for the motor, holding all the state required for operation and calibration.
pub struct MotorDriver {
    motor: MotorPWM,    // Motor interface using PWM signals for control
    frequency: u16,     // Update frequency (ticks per second)
    pwm: [i16; 4],      // Current PWM output sent to the motor
    position: Position, // Current encoder position reading

    motor_status: MotorStatus, // Current motor status (Calibrating, Ready, or Error)

    angle_el: u16,  // Electrical angle of the motor (0..65535), used to control phase
    amplitude: i16, // Amplitude (voltage magnitude) used during calibration
    direction: i16, // Current rotation direction (1 for forward, -1 for backward)
    speed: i16,     // Speed (steps per tick) during calibration

    angle_calibrator: AngleCalibrator,
    filter: FilterLPF,
    supply: SupplyVoltage,
    ticker:i32,    
}

// Constants used during calibration
impl MotorDriver {
    /// Create a new MotorDriver instance.
    ///
    /// # Arguments
    /// * `motor` - Motor type configuration
    /// * `connection` - Phase pattern configuration
    /// * `frequency` - Number of ticks per second
    pub fn new(
        motor: MotorType,
        connection: PhasePattern,
        frequency: u16,
        max_sup_voltage: i32,
    ) -> Self {
        Self {
            motor: MotorPWM::new(motor, connection), // Initialize MotorPWM with given type and phase connection
            frequency,                               // Store the update frequency
            position: Position::new(),               // Initialize encoder position to 0

            motor_status: MotorStatus::Calibrating, // Start in Calibrating mode

            angle_el: 0, // Initial electrical angle is 0

            pwm: [0; 4],     // Initialize PWM outputs to zero
            amplitude: 0, 

            direction: 0, // No direction initially
            speed: 0,     // Use the predefined calibration speed

            angle_calibrator: AngleCalibrator::new(frequency),
            filter: FilterLPF::new(0, 0),

            supply: SupplyVoltage::new(200, max_sup_voltage),
            ticker: 0
        }
    }

    //---------------------------------------------------------
    // tick() Method Steps:
    //
    // 1. Update the encoder position from the given input.
    // 2. If the motor is ready (calibration done), run normal logic via tick_run().
    // 3. If the motor is in error state, set amplitude to 0.
    // 4. Otherwise (still calibrating), run the calibration logic via tick_calibrate().
    // 5. After updating angle and amplitude based on the chosen logic, compute the final PWM signals.
    //---------------------------------------------------------

    // pub fn update(&mut self, voltage_on_motor: i32, encoder_pos: u16, supply: u16) {
    //     self.
    // }

    /// Main update method.
    ///
    /// # Arguments
    /// * `voltg_angle` - (angle, amplitude) tuple for normal operation
    /// * `encoder_pos` - current encoder position from the sensor
    ///
    /// This method decides whether to run normal operation or calibration logic based on the motor status.
    pub fn tick(&mut self, voltage_on_motor: i32, encoder_pos: u16, supply: u16) -> [i16; 4] {
        self.position.tick(encoder_pos); // Update the internal position from the sensor
        let voltage_mv = self.supply.tick(supply).voltage_mv();
        let duty = (voltage_on_motor << 15) / (voltage_mv + 1);
        self.amplitude = if duty > i16::MAX as i32 {i16::MAX} else {duty as i16};
        match self.motor_status {
            MotorStatus::Ready => {
                self.ticker += 1;
                
                // If calibration is complete, run normal operation logic
                let filtered_pos = self.filter.tick(self.position.angle());

                self.angle_el = self
                    .angle_calibrator
                    .get_correction(filtered_pos)
                    .1;
            }
            MotorStatus::Error => {
                // If in error state, stop driving the motor by setting amplitude to 0
                self.amplitude = 0;
            }
            MotorStatus::Calibrating => {
                // If still calibrating, run the calibration logic
                self.angle_el = self.angle_calibrator.tick(self.position.position());
                if self.angle_calibrator.is_ready() {
                    self.motor_status = MotorStatus::Ready
                }
            }
        }

        // Compute the PWM signals based on the current angle_el and amplitude
        self.pwm = self
            .motor
            .tick_angle((self.angle_el as i16, self.amplitude));
        self.pwm // Return the updated PWM array
    }

    //---------------------------------------------------------
    // is_ready() Method Steps:
    //
    // 1. Simply returns true if the motor_status is Ready.
    //---------------------------------------------------------

    /// Check if calibration is complete (motor is ready for normal operation).
    pub fn is_ready(&self) -> bool {
        matches!(self.motor_status, MotorStatus::Ready) // Returns true if Ready
    }

    //---------------------------------------------------------
    // change_motor_mode() and change_phase_mode() Steps:
    //
    // 1. These methods allow changing the motor mode and phase pattern at runtime.
    // 2. They simply delegate the call to the MotorPWM instance.
    //---------------------------------------------------------

    /// Change the motor type mode.
    #[inline(always)]
    pub fn change_motor_mode(&mut self, motor: MotorType) {
        self.motor.change_motor_mode(motor); // Delegate to motor instance
    }

    /// Change the phase pattern mode.
    #[inline(always)]
    pub fn change_phase_mode(&mut self, connection: PhasePattern) {
        self.motor.change_phase_mode(connection); // Delegate to motor instance
    }

    //---------------------------------------------------------
    // get_pwm() Method Steps:
    //
    // 1. Returns the current PWM output array.
    //---------------------------------------------------------

    /// Get current PWM signals.
    #[inline(always)]
    pub fn get_pwm(&mut self) -> [i16; 4] {
        self.pwm // Return the current PWM array
    }
}
