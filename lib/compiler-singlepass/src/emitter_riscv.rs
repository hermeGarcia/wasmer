//! RISC-V emitter scaffolding.

use crate::{
    codegen_error, common_decl::Size, location::Location as AbstractLocation,
    machine_riscv::AssemblerRiscv,
};
pub use crate::{
    location::Multiplier,
    machine::{Label, Offset},
    riscv_decl::{FPR, GPR},
};
use dynasm::dynasm;
use dynasmrt::riscv::RiscvRelocation;
use dynasmrt::{AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, VecAssembler};

use wasmer_compiler::types::function::FunctionBody;
use wasmer_types::FunctionType;
use wasmer_types::CompileError;
use wasmer_types::target::{CpuFeature, CallingConvention};

/// Force `dynasm!` to use the correct arch (riscv64) when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a
            ; .arch riscv64
            ; $($tt)*
        )
    };
}

type Assembler = VecAssembler<RiscvRelocation>;

/// Location abstraction specialized to RISC-V.
pub type Location = AbstractLocation<GPR, FPR>;

/// Branch conditions for RISC-V.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Condition {
    // TODO: define RISC-V branch conditions.
}

/// Emitter trait for RISC-V.
#[allow(unused)]
pub trait EmitterRiscv {
    /// Returns the SIMD (FPU) feature if available.
    fn get_simd_arch(&self) -> Option<&CpuFeature>;
    /// Generates a new internal label.
    fn get_label(&mut self) -> Label;
    /// Gets the current code offset.
    fn get_offset(&self) -> Offset;
    /// Returns the size of a jump instruction in bytes.
    fn get_jmp_instr_size(&self) -> u8;

    /// Finalize the function, e.g., resolve labels.
    fn finalize_function(&mut self) -> Result<(), CompileError>;

    // TODO: add methods for emitting RISC-V instructions (e.g., loads, stores, arithmetic, branches, etc.)
}

impl EmitterRiscv  for Assembler {
    fn get_simd_arch(&self) -> Option<&CpuFeature> {
        todo!()
    }

    fn get_label(&mut self) -> Label {
        todo!()
    }

    fn get_offset(&self) -> Offset {
        self.offset()
    }

    fn get_jmp_instr_size(&self) -> u8 {
        todo!()
    }

    fn finalize_function(&mut self) -> Result<(), CompileError> {
        todo!()
    }
}


pub fn gen_std_trampoline_riscv64(
    sig: &FunctionType,
    calling_convention: CallingConvention,
) -> Result<FunctionBody, CompileError> {
    let mut assembler = Assembler::new(0);


    let fptr = GPR::X8;
    let args = GPR::X12;

    dynasm!(assembler
        // ; addi sp, sp, -32
        // ; sw x29, [sp]
        // ; sw x30, [sp, 8]
        // ; sw X(fptr as u32), [sp, 16]
        // ; sw X(args as u32), [sp, 32]
        // ; mv x29, sp
        ; mv X(fptr as u32), x1
        ; mv X(args as u32), x2

        ;jalr X(fptr as u32)

        // ; mv x28, s2
        // ; sw a0, [s2]
        ; jr ra
    );


    let mut body = assembler.finalize().unwrap();
    body.shrink_to_fit();

    Ok(FunctionBody {
        body,
        unwind_info: None,
    })
}