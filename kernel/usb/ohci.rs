use alloc::boxed::Box;

use collections::vec::Vec;

use core::intrinsics::volatile_load;
use core::{mem, slice};

use drivers::mmio::Mmio;
use drivers::pciconfig::PciConfig;

use schemes::KScheme;

use super::hci::{UsbHci, UsbMsg};
use super::setup::Setup;

#[repr(packed)]
#[derive(Copy, Clone, Debug, Default)]
struct Gtd {
    flags: u32,
    buffer: u32,
    next: u32,
    end: u32,
}

#[repr(packed)]
#[derive(Copy, Clone, Debug, Default)]
struct Ed {
    flags: u32,
    tail: u32,
    head: u32,
    next: u32,
}

const CTRL_CBSR: u32 = 0b11;
const CTRL_PLE: u32 = 1 << 2;
const CTRL_IE: u32 = 1 << 3;
const CTRL_CLE: u32 = 1 << 4;
const CTRL_BLE: u32 = 1 << 5;
const CTRL_HCFS: u32 = 0b11 << 6;
const CTRL_IR: u32 = 1 << 8;
const CTRL_RWC: u32 = 1 << 9;
const CTRL_RWE: u32 = 1 << 10;

const CMD_STS_HCR: u32 = 1;
const CMD_STS_CLF: u32 = 1 << 1;
const CMD_STS_BLF: u32 = 1 << 2;
const CMD_STS_OCR: u32 = 1 << 3;

const PORT_STS_CCS: u32 = 1;
const PORT_STS_PES: u32 = 1 << 1;
const PORT_STS_PSS: u32 = 1 << 2;
const PORT_STS_POCI: u32 = 1 << 3;
const PORT_STS_PPS: u32 = 1 << 8;
const PORT_STS_LSDA: u32 = 1 << 9;
const PORT_STS_CSC: u32 = 1 << 16;
const PORT_STS_PESC: u32 = 1 << 17;
const PORT_STS_PSSC: u32 = 1 << 18;
const PORT_STS_OCIC: u32 = 1 << 19;
const PORT_STS_PRSC: u32 = 1 << 20;

#[repr(packed)]
pub struct OhciRegs {
    pub revision: Mmio<u32>,
    pub control: Mmio<u32>,
    pub cmd_sts: Mmio<u32>,
    pub int_sts: Mmio<u32>,
    pub int_en: Mmio<u32>,
    pub int_dis: Mmio<u32>,
    pub hcca: Mmio<u32>,
    pub period_current: Mmio<u32>,
    pub control_head: Mmio<u32>,
    pub control_current: Mmio<u32>,
    pub bulk_head: Mmio<u32>,
    pub bulk_current: Mmio<u32>,
    pub done_head: Mmio<u32>,
    pub fm_interval: Mmio<u32>,
    pub fm_remain: Mmio<u32>,
    pub fm_num: Mmio<u32>,
    pub periodic_start: Mmio<u32>,
    pub ls_thresh: Mmio<u32>,
    pub rh_desc_a: Mmio<u32>,
    pub rh_desc_b: Mmio<u32>,
    pub rh_sts: Mmio<u32>,
    pub port_sts: [Mmio<u32>; 15],
}

pub struct Ohci {
    pub regs: &'static mut OhciRegs,
    pub irq: u8,
}

impl KScheme for Ohci {
    fn on_irq(&mut self, irq: u8) {
        if irq == self.irq {
            // d("OHCI IRQ\n");
        }
    }

    fn on_poll(&mut self) {
    }
}

impl Ohci {
    pub unsafe fn new(mut pci: PciConfig) -> Box<Self> {
        pci.flag(4, 4, true); // Bus mastering

        let base = pci.read(0x10) as usize & 0xFFFFFFF0;
        let regs = &mut *(base as *mut OhciRegs);

        let mut module = box Ohci {
            regs: regs,
            irq: pci.read(0x3C) as u8 & 0xF,
        };

        module.init();

        return module;
    }

    pub unsafe fn init(&mut self) {
        debugln!("OHCI on: {:X}, IRQ: {:X}", (self.regs as *mut OhciRegs) as usize, self.irq);

        let ctrl = self.regs.control.read();
        self.regs.control.write(ctrl & (0xFFFFFFFF - CTRL_HCFS) | 0b10 << 6);

        let ndp = self.regs.rh_desc_a.read() & 0xF;
        for i in 0..ndp as usize {
            debugln!("Port {}: {:X}", i, self.regs.port_sts[i].read());

            if self.regs.port_sts[i].readf(PORT_STS_CCS) {
                debugln!("Device");

                debugln!("Enable");
                while ! self.regs.port_sts[i].readf(PORT_STS_PES) {
                    self.regs.port_sts[i].writef(PORT_STS_PES, true);
                }

                self.device(i as u8);
            }
        }
    }
}


impl UsbHci for Ohci {
    fn msg(&mut self, address: u8, endpoint: u8, msgs: &[UsbMsg]) -> usize {
        let mut tds = Vec::new();
        for msg in msgs.iter().rev() {
            let link_ptr = match tds.last() {
                Some(td) => (td as *const Gtd) as u32,
                None => 0
            };

            match *msg {
                UsbMsg::Setup(setup) => tds.push(Gtd {
                    flags: 0b1111 << 28 | 0b00 << 19,
                    buffer: (setup as *const Setup) as u32,
                    next: link_ptr,
                    end: (setup as *const Setup) as u32 + mem::size_of::<Setup>() as u32
                }),
                UsbMsg::In(ref data) => tds.push(Gtd {
                    flags: 0b1111 << 28 | 0b10 << 19,
                    buffer: data.as_ptr() as u32,
                    next: link_ptr,
                    end: data.as_ptr() as u32 + data.len() as u32
                }),
                UsbMsg::InIso(ref data) => tds.push(Gtd {
                    flags: 0b1111 << 28 | 0b10 << 19,
                    buffer: data.as_ptr() as u32,
                    next: link_ptr,
                    end: data.as_ptr() as u32 + data.len() as u32
                }),
                UsbMsg::Out(ref data) => tds.push(Gtd {
                    flags: 0b1111 << 28 | 0b01 << 19,
                    buffer: data.as_ptr() as u32,
                    next: link_ptr,
                    end: data.as_ptr() as u32 + data.len() as u32
                }),
                UsbMsg::OutIso(ref data) => tds.push(Gtd {
                    flags: 0b1111 << 28 | 0b01 << 19,
                    buffer: data.as_ptr() as u32,
                    next: link_ptr,
                    end: data.as_ptr() as u32 + data.len() as u32
                })
            }
        }

        let mut count = 0;

        if ! tds.is_empty() {
            let ed = box Ed {
                flags: 1024 << 16 | (endpoint as u32) << 7 | address as u32,
                tail: 0,
                head: (tds.last().unwrap() as *const Gtd) as u32,
                next: 0
            };

            //TODO: Calculate actual bytes
            for td in tds.iter().rev() {
                count += (td.end - td.buffer) as usize;
            }

            /*
            self.regs.control_head.write((&*ed as *const Ed) as u32);
            while ! self.regs.control.readf(CTRL_CLE) {
                self.regs.control.writef(CTRL_CLE, true);
            }
            while ! self.regs.cmd_sts.readf(CMD_STS_CLF) {
                self.regs.cmd_sts.writef(CMD_STS_CLF, true);
            }

            for td in tds.iter().rev() {
                while unsafe { volatile_load(td as *const Gtd).flags } & 0b1111 << 28 == 0b1111 << 28 {
                    //unsafe { context_switch(false) };
                }
            }

            while self.regs.cmd_sts.readf(CMD_STS_CLF) {
                self.regs.cmd_sts.writef(CMD_STS_CLF, false);
            }
            while self.regs.control.readf(CTRL_CLE) {
                self.regs.control.writef(CTRL_CLE, false);
            }
            self.regs.control_head.write(0);
            */
        }

        count
    }
}