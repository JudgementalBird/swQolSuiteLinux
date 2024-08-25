use std::{arch::asm, sync::atomic::Ordering};

use anyhow::Context;
use atomic_float::AtomicF32;
use hudhook::imgui::TreeNodeFlags;
use memory_rs::{
    generate_aob_pattern,
    internal::{
        injections::{Inject, Injection},
        memory_region::MemoryRegion,
    },
};

use super::{MemoryRegionExt, Tweak};

const VANILLA_BASE_SPEED: f32 = 1.0;
const DEFAULT_BASE_SPEED: f32 = 0.8;

const VANILLA_SHIFT_MULTIPLIER: f32 = 0.2;
const DEFAULT_SHIFT_MULTIPLIER: f32 = 3.0;

const VANILLA_CONTROL_MULTIPLIER: f32 = 1.0;
const DEFAULT_CONTROL_MULTIPLIER: f32 = 0.2;

const VANILLA_WHEEL_MULTIPLIER: f32 = 1.0;
const DEFAULT_WHEEL_MULTIPLIER: f32 = 1.1;

static SPEED: AtomicF32 = AtomicF32::new(1.0);

#[no_mangle]
static mut jmp_back_addr: usize = 0x0;

pub struct EditorCameraSpeedTweak {
    base_speed: f32,
    shift_multiplier: f32,
    control_multiplier: f32,
    wheel_multiplier: f32,

    current_wheel_multiplier: f32,
    current_speed: f32,
    _speed_inject: Injection,
}

impl EditorCameraSpeedTweak {
    pub fn new(region: &MemoryRegion) -> anyhow::Result<Self> {
        // `xmm5 = param_1->shift_down ? 0.2 : 1.0`
        #[rustfmt::skip]
        let memory_pattern = generate_aob_pattern![
            0x80, 0xb9, 0x1a, 0x0e, 0x00, 0x00, 0x00, // CMP      byte ptr [RCX + param_1->shift_down],0x0
            0x74, 0x0a,                               // JZ       +0xA
            0xf3, 0x0f, 0x10, 0x2d, _, _, _, _,       // MOVSS    XMM5,dword ptr [FLOAT_XXX]    = 0.2
            0xeb, 0x08,                               // JMP      +0x8
            0xf3, 0x0f, 0x10, 0x2d, _, _, _, _        // MOVSS    XMM5,dword ptr [FLOAT_XXX]    = 1.0
        ];
        let speed_addr = {
            region
                .scan_aob_single(&memory_pattern)
                .context("Error finding editor camera speed addr")?
        };

        // CALL custom_speed
        let mut inject = vec![0xff, 0x15, 0x02, 0x00, 0x00, 0x00, 0xeb, 0x08];
        inject.extend_from_slice(&(custom_speed as usize).to_le_bytes());
        // pad with NOP
        inject.resize(memory_pattern.size, 0x90);

        let mut speed_inject = Injection::new(speed_addr, inject);
        speed_inject.inject();

        Ok(Self {
            base_speed: DEFAULT_BASE_SPEED,
            shift_multiplier: DEFAULT_SHIFT_MULTIPLIER,
            control_multiplier: DEFAULT_CONTROL_MULTIPLIER,
            wheel_multiplier: DEFAULT_WHEEL_MULTIPLIER,

            current_wheel_multiplier: 1.0,
            current_speed: DEFAULT_BASE_SPEED,
            _speed_inject: speed_inject,
        })
    }
}

impl Tweak for EditorCameraSpeedTweak {
    fn uninit(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(&mut self, ui: &hudhook::imgui::Ui) {
        if ui.collapsing_header("Editor Camera", TreeNodeFlags::empty()) {
            ui.set_next_item_width(100.0);
            ui.slider("Base Speed", 0.1, 4.0, &mut self.base_speed);
            if ui.is_item_hovered() {
                ui.tooltip_text(format!(
                    "(default: {DEFAULT_BASE_SPEED}, vanilla: {VANILLA_BASE_SPEED})"
                ));
            }

            ui.set_next_item_width(100.0);
            ui.slider("Shift Multiplier", 0.1, 4.0, &mut self.shift_multiplier);
            if ui.is_item_hovered() {
                ui.tooltip_text(format!(
                    "(default: {DEFAULT_SHIFT_MULTIPLIER}, vanilla: {VANILLA_SHIFT_MULTIPLIER})"
                ));
            }

            ui.set_next_item_width(100.0);
            ui.slider("Ctrl Multiplier", 0.1, 4.0, &mut self.control_multiplier);
            if ui.is_item_hovered() {
                ui.tooltip_text(format!("(default: {DEFAULT_CONTROL_MULTIPLIER}, vanilla: {VANILLA_CONTROL_MULTIPLIER})"));
            }

            // ui.set_next_item_width(100.0);
            // ui.slider("Scroll Multiplier", 0.1, 4.0, &mut self.wheel_multiplier);
            // if ui.is_item_hovered() {
            //     ui.tooltip_text(format!("(default: {DEFAULT_WHEEL_MULTIPLIER}, vanilla: {VANILLA_WHEEL_MULTIPLIER})"));
            // }

            // ui.text(format!("{}", self.current_speed));
            // ui.text(format!("{}", self.current_wheel_multiplier));
        }
    }

    fn constant_render(&mut self, ui: &hudhook::imgui::Ui) {
        // if ui.io().mouse_wheel != 0.0 {
        //     self.current_wheel_multiplier *= self.wheel_multiplier.powf(ui.io().mouse_wheel);
        // }

        self.current_speed = self.base_speed * self.current_wheel_multiplier;

        if ui.is_key_down(hudhook::imgui::Key::LeftShift)
            || ui.is_key_down(hudhook::imgui::Key::RightShift)
        {
            self.current_speed *= self.shift_multiplier;
        }

        if ui.is_key_down(hudhook::imgui::Key::LeftCtrl)
            || ui.is_key_down(hudhook::imgui::Key::RightCtrl)
        {
            self.current_speed *= self.control_multiplier;
        }

        SPEED.store(self.current_speed, Ordering::Release);
    }

    fn reset_to_default(&mut self) {
        self.base_speed = DEFAULT_BASE_SPEED;
        self.shift_multiplier = DEFAULT_SHIFT_MULTIPLIER;
        self.control_multiplier = DEFAULT_CONTROL_MULTIPLIER;
        self.wheel_multiplier = DEFAULT_WHEEL_MULTIPLIER;
    }

    fn reset_to_vanilla(&mut self) {
        self.base_speed = VANILLA_BASE_SPEED;
        self.shift_multiplier = VANILLA_SHIFT_MULTIPLIER;
        self.control_multiplier = VANILLA_CONTROL_MULTIPLIER;
        self.wheel_multiplier = VANILLA_WHEEL_MULTIPLIER;
    }
}

#[no_mangle]
extern "stdcall" fn custom_speed() {
    unsafe {
        // put speed value in xmm5
        asm!(
            "",
            in("xmm5") SPEED.load(Ordering::Acquire),
            options(nostack),
        );
    }
}
