/// NES joypad — buttons read serially after strobe.
///
/// Button bit layout: A B Select Start Up Down Left Right
pub struct Joypad {
    pub strobe: bool,
    pub button_index: u8,
    pub button_state: u8,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoypadButton {
    A      = 0b0000_0001,
    B      = 0b0000_0010,
    Select = 0b0000_0100,
    Start  = 0b0000_1000,
    Up     = 0b0001_0000,
    Down   = 0b0010_0000,
    Left   = 0b0100_0000,
    Right  = 0b1000_0000,
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            strobe: false,
            button_index: 0,
            button_state: 0,
        }
    }

    /// Write to $4016 — controls strobe.
    pub fn write(&mut self, val: u8) {
        self.strobe = (val & 1) == 1;
        self.button_index = 0;
    }

    /// Read from $4016/$4017 — returns one button bit per read.
    pub fn read(&mut self) -> u8 {
        if self.button_index > 7 {
            return 1; // after 8 reads, return 1
        }
        let result = (self.button_state >> self.button_index) & 1;
        if !self.strobe {
            self.button_index += 1;
        }
        result
    }

    /// Set the pressed state of a single button.
    pub fn set_button(&mut self, button: JoypadButton, pressed: bool) {
        if pressed {
            self.button_state |= button as u8;
        } else {
            self.button_state &= !(button as u8);
        }
    }

    /// Set the pressed state of all buttons at once.
    pub fn set_buttons(&mut self, state: u8) {
        self.button_state = state;
    }
}
