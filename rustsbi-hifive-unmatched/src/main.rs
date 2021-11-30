#![no_std]
#![no_main]
#![feature(naked_functions)]
#![feature(asm, asm_const, asm_sym)]
#![feature(generator_trait)]
#![feature(default_alloc_error_handler)]
#![no_std]
#![no_main]

mod runtime;
mod peripheral;
mod early_trap;

use core::panic::PanicInfo;
use rustsbi::println;

#[panic_handler]
fn on_panic(panic_info: &PanicInfo) -> ! {
    eprintln!("panic occurred: {}", panic_info);
    loop {}
}

fn rust_main(hartid: usize, opaque: usize) -> ! {
    if hartid == 1 { // 不应该用第0个核启动，它不支持LR/SC指令（SiFive S7核文档，第3.6节），不能使用部分的异步锁。感谢@dram
        init_bss();
    }
    if hartid == 1 {
        let uart = unsafe { peripheral::Uart::prev_bootloading_step() };
        init_stdout(uart);
        early_trap::init(hartid);
        eprintln!("rustsbi: hello world! hart id: {:x}, opaque: {:x}", hartid, opaque);
        print_misa();
        init_heap(); // 必须先加载堆内存，才能使用rustsbi框架
        init_stdio();
        println!("rustsbi: hello world!");
        println!("rustsbi: hello world! {:x} {:x}", hartid, opaque);
    }
    runtime::init();
    loop {}
}

fn init_bss() {
    extern "C" {
        static mut ebss: u32;
        static mut sbss: u32;
        static mut edata: u32;
        static mut sdata: u32;
        static sidata: u32;
    }
    unsafe {
        r0::zero_bss(&mut sbss, &mut ebss);
        r0::init_data(&mut sdata, &mut edata, &sidata);
    } 
}

fn init_stdio() {
    use rustsbi::legacy_stdio::init_legacy_stdio_embedded_hal;
    let uart = unsafe { peripheral::Uart::prev_bootloading_step() };
    init_legacy_stdio_embedded_hal(uart);
}

#[inline]
fn print_misa() {
    use riscv::register::misa::{self, MXL};
    let isa = misa::read();
    if let Some(isa) = isa {
        let mxl_str = match isa.mxl() {
            MXL::XLEN32 => "RV32",
            MXL::XLEN64 => "RV64",
            MXL::XLEN128 => "RV128",
        };
        eprint!("[rustsbi] misa: {}", mxl_str);
        for ext in 'A'..='Z' {
            if isa.has_extension(ext) {
                eprint!("{}", ext);
            }
        }
        eprintln!("");
    }
}

const SBI_HEAP_SIZE: usize = 64 * 1024; // 64KiB
#[link_section = ".bss.uninit"]
static mut HEAP_SPACE: [u8; SBI_HEAP_SIZE] = [0; SBI_HEAP_SIZE];

use buddy_system_allocator::LockedHeap;

use crate::peripheral::uart::init_stdout;
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::empty();

#[inline] fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_SPACE.as_ptr() as usize, SBI_HEAP_SIZE);
    }
}

const PER_HART_STACK_SIZE: usize = 4 * 4096; // 16KiB
const SBI_STACK_SIZE: usize = 5 * PER_HART_STACK_SIZE; // 5 harts
#[link_section = ".bss.uninit"]
static mut SBI_STACK: [u8; SBI_STACK_SIZE] = [0; SBI_STACK_SIZE];

#[naked]
#[link_section = ".text.entry"]
#[export_name = "_start"]
unsafe extern "C" fn entry() -> ! {
    asm!(
    // 1. clear all registers
    "li x1, 0
    li x2, 0
    li x3, 0
    li x4, 0
    li x5, 0
    li x6, 0
    li x7, 0
    li x8, 0
    li x9, 0",
    // no x10 and x11: x10 is a0 and x11 is a1, they are passed to 
    // main function as arguments
    "li x12, 0
    li x13, 0
    li x14, 0
    li x15, 0
    li x16, 0
    li x17, 0
    li x18, 0
    li x19, 0
    li x20, 0
    li x21, 0
    li x22, 0
    li x23, 0
    li x24, 0
    li x25, 0
    li x26, 0
    li x27, 0
    li x28, 0
    li x29, 0
    li x30, 0
    li x31, 0",
    // 2. set sp
    // sp = bootstack + (hartid + 1) * HART_STACK_SIZE
    "
    la      sp, {stack}
    li      t0, {per_hart_stack_size}
    csrr    t1, mhartid
    addi    t2, t1, 1
1:  add     sp, sp, t0
    addi    t2, t2, -1
    bnez    t2, 1b
    ",
    // 3. jump to main function (absolute address)
    "j      {rust_main}",
    per_hart_stack_size = const PER_HART_STACK_SIZE,
    stack = sym SBI_STACK,
    rust_main = sym rust_main,
    options(noreturn))
}
