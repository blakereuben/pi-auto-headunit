//! Night-mode input via a single Linux GPIO character-device line
//! (`/dev/gpiochipN`, the modern `gpiod` uAPI — Pi 5's RP1 southbridge
//! exposes its GPIOs this way, not the deprecated `/sys/class/gpio`
//! sysfs interface).
//!
//! # Wiring safety
//!
//! Raspberry Pi GPIO pins are **3.3V logic and are not 5V tolerant**.
//! Connecting a car's 12V (or a relay's switched 5V) circuit directly to
//! a GPIO pin can permanently damage it. Use an opto-isolator, or at
//! minimum a resistor divider sized to output 3.3V max, between the
//! car's illumination-wire relay and the chosen GPIO. Designing that
//! external circuit is outside this repository's scope (software only);
//! this module only reads the resulting 3.3V-logic digital line.

use std::io;
use std::path::Path;

use gpiod::{Active, Bias, Chip, Input, Lines, Options};

/// The GPIO chip device most Raspberry Pi boards expose their 40-pin
/// header lines through.
pub const DEFAULT_GPIO_CHIP: &str = "/dev/gpiochip0";

/// A single GPIO line configured as a pull-down digital input, read to
/// detect the car's illumination (headlight) signal for night mode.
///
/// Pulled down (not left floating/disabled) so an unconnected or
/// not-yet-wired line reads a clean, stable `false` (day mode) rather
/// than an undefined value.
pub struct NightModeGpioSource {
    lines: Lines<Input>,
}

impl NightModeGpioSource {
    pub fn open(chip_path: &Path, line: u32) -> io::Result<Self> {
        let chip = Chip::new(chip_path)?;
        let lines = chip.request_lines(
            Options::input([line])
                .consumer("aa-headunit-night-mode")
                .active(Active::High)
                .bias(Bias::PullDown),
        )?;
        Ok(Self { lines })
    }

    /// Reads the line's current state. `true` means the signal is
    /// active (headlights on, per the wiring this module's doc comment
    /// describes) — night mode should be on.
    pub fn is_active(&self) -> io::Result<bool> {
        let values: u8 = self.lines.get_values(0u8)?;
        Ok(values & 1 != 0)
    }
}
