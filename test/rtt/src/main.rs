#![no_main]
#![no_std]

use cortex_m_rt::entry;
use hal::pac;
use libm::{exp, floorf, sin, sqrtf};
use panic_halt as _;

#[repr(C, packed)]
struct RawDataPoint {
    id: u8,
    timestamp: u32,
    value: f32,
}

use hal::{
    clocks::Clocks,
    gpio::{Edge, Pin, PinMode, Port, Pull},
    pac::TIM3,
    timer::{
        Alignment, CaptureCompareDma, CountDir, OutputCompare, TimChannel, Timer, TimerConfig,
        TimerInterrupt, UpdateReqSrc,
    },
};

use rtt_target::{rtt_init, ChannelMode::NoBlockSkip};

const STRUCT_SIZE: usize = core::mem::size_of::<RawDataPoint>();
const BUFFER_MULTIPLE: usize = 8; // Number of RawDataPoints to buffer
const BUFFER_SIZE: usize = STRUCT_SIZE * BUFFER_MULTIPLE;

#[entry]
fn main() -> ! {
    // Initialize RTT with up channel configured for binary data
    let channels = rtt_init! {
        up: {
            0: {
                size: BUFFER_SIZE,
                mode: NoBlockSkip,
                name: "Up",
            }
        }
    };

    let mut up = channels.up.0;

    let mut led_green = Pin::new(Port::B, 14, PinMode::Output);

    const FREQUENCY: f32 = 1_000_000.0;
    // Set up microcontroller peripherals
    let _dp = pac::Peripherals::take().unwrap();

    /// Initialize the clocks for the microcontroller
    let clock_cfg = hal::clocks::Clocks::default();
    clock_cfg.setup().unwrap();

    // Create a new Timer with the specified frequency and configuration
    let mut timer_pwd = Timer::new_tim3(_dp.TIM3, FREQUENCY, TimerConfig::default(), &clock_cfg);
    // Enable update interrupt for the timer
    timer_pwd.set_auto_reload(0xFFFF);
    timer_pwd.enable();

    let mut counter = 0;
    let max_count = 10_000;
    let mut tick: u64 = 0;

    loop {
        let data_point = RawDataPoint {
            id: 0,
            timestamp: tick as u32,
            value: counter as f32,
        };

        // make one with a sine wave
        let sine_data_point = RawDataPoint {
            id: 1,
            timestamp: tick as u32,
            value: (sin(tick as f64 * 0.001) * max_count as f64) as f32,
        };

        // Send raw bytes directly through RTT
        unsafe {
            let bytes = core::slice::from_raw_parts(
                &data_point as *const RawDataPoint as *const u8,
                core::mem::size_of::<RawDataPoint>(),
            );
            up.write(bytes);
        }
        unsafe {
            let bytes = core::slice::from_raw_parts(
                &sine_data_point as *const RawDataPoint as *const u8,
                core::mem::size_of::<RawDataPoint>(),
            );
            up.write(bytes);
        }

        // Blink the green LED
        if counter >= max_count {
            led_green.toggle();
            counter = 0;
        }
        counter += 1;
        tick += 1;
    }
}
