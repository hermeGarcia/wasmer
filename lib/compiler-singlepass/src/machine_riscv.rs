//! RISC-V machine scaffolding.
//! 
use dynasmrt::{riscv::RiscvRelocation, DynasmError, VecAssembler};
#[cfg(feature = "unwind")]
use gimli::{write::CallFrameInstruction, RiscV};

use wasmer_compiler::{
    types::{
        address_map::InstructionAddressMap,
        function::FunctionBody,
        relocation::{Relocation, RelocationKind, RelocationTarget},
        section::{CustomSection, CustomSectionProtection, SectionBody},
    },
    wasmparser::{MemArg, ValType as WpType},
};
use wasmer_types::{
    target::{CallingConvention, CpuFeature, Target},
    CompileError, FunctionIndex, FunctionType, SourceLoc, TrapCode, TrapInformation, VMOffsets,
};

use crate::{
    codegen_error,
    common_decl::*,
    emitter_riscv::*,
    location::{Location as AbstractLocation, Reg},
    machine::*,
    riscv_decl::{new_machine_state, FPR, GPR},
    unwind::{UnwindInstructions, UnwindOps},
};

type Assembler = VecAssembler<RiscvRelocation>;
type Location = AbstractLocation<GPR, FPR>;

/// Force `dynasm!` to use the correct arch (riscv64) when cross-compiling.
macro_rules! dynasm {
    ($a:expr ; $($tt:tt)*) => {
        dynasm::dynasm!(
            $a.inner
            ; .arch riscv64
            ; $($tt)*
        )
    };
}

use std::ops::{Deref, DerefMut};
/// The RISC-V assembler wrapper, providing FPU feature tracking and a dynasmrt assembler.
pub struct AssemblerRiscv {
    /// Inner dynasm assembler.
    pub inner: Assembler,
    /// Optional FPU (SIMD) feature.
    pub simd_arch: Option<CpuFeature>,
    /// Target CPU configuration.
    pub target: Option<Target>,
}

impl AssemblerRiscv {
    /// Create a new RISC-V assembler.
    pub fn new(base_addr: usize, target: Option<Target>) -> Result<Self, CompileError> {
        let simd_arch = None; // TODO: detect RISC-V FPU extensions (e.g., F/D)
        Ok(Self {
            inner: Assembler::new(base_addr),
            simd_arch,
            target,
        })
    }

    /// Finalize to machine code bytes.
    pub fn finalize(mut self) -> Result<Vec<u8>, DynasmError> {
        // Sum two numbers in RISC-V
        dynasm!(self
            ; .arch riscv64
            ; addi x1, x1, 1
            ; addi x2, x2, 2
            ; add x3, x1, x2
            ; ret
        );
        let bytes = self.inner.finalize();
        bytes
    }
}

impl Deref for AssemblerRiscv {
    type Target = Assembler;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for AssemblerRiscv {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[cfg(feature = "unwind")]
fn dwarf_index(reg: u16) -> gimli::Register {
    // TODO: map DWARF register numbers for RISC-V.
    todo!()
}

/// The RISC-V machine state and code emitter.
pub struct MachineRiscv {
    assembler: Assembler,
    used_gprs: u32,
    used_simd: u32,
    trap_table: TrapTable,
    /// Map from byte offset into wasm function to range of native instructions.
    /// Ordered by increasing InstructionAddressMap::srcloc.
    instructions_address_map: Vec<InstructionAddressMap>,
    /// The source location for the current operator.
    src_loc: u32,
    /// Vector of unwind operations with offset.
    unwind_ops: Vec<(usize, UnwindOps)>,
    /// Flag indicating if this machine supports floating-point.
    has_fpu: bool,
}

impl MachineRiscv {
    /// Creates a new RISC-V machine for code generation.
    pub fn new(target: Option<Target>) -> Self {
        let has_fpu = match target {
            Some(ref t) => t.cpu_features().contains(CpuFeature::NEON), // TODO: replace with RISC-V FPU feature
            None => false,
        };
        MachineRiscv {
            assembler: Assembler::new(0),
            used_gprs: 0,
            used_simd: 0,
            trap_table: TrapTable::default(),
            instructions_address_map: vec![],
            src_loc: 0,
            unwind_ops: vec![],
            has_fpu,
        }
    }
}

#[allow(dead_code)]
#[derive(PartialEq)]
enum ImmType {
    // TODO: define RISC-V immediate types.
}

#[allow(dead_code)]
impl MachineRiscv {
    // TODO: helper functions for RISC-V immediates and addressing.

    fn used_gprs_contains(&self, r: &GPR) -> bool {
        self.used_gprs & (1 << r.into_index()) != 0
    }

    fn used_gprs_insert(&mut self, r: GPR) {
        self.used_gprs |= 1 << r.into_index();
    }

    fn used_gprs_remove(&mut self, r: &GPR) -> bool {
        let ret = self.used_gprs_contains(r);
        self.used_gprs &= !(1 << r.into_index());
        ret
    }
}

impl Machine for MachineRiscv {
    type GPR = GPR;
    type SIMD = FPR;
    fn assembler_get_offset(&self) -> Offset {
        self.assembler.get_offset()
    }
    fn index_from_gpr(&self, x: Self::GPR) -> RegisterIndex {
        RegisterIndex(x as usize)
    }
    fn index_from_simd(&self, x: Self::SIMD) -> RegisterIndex {
        todo!()
    }
    fn get_vmctx_reg(&self) -> Self::GPR {
        GPR::X27
    }
    fn pick_gpr(&self) -> Option<Self::GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[X26, X25, X24, X23, X22, X21, X20, X19, X18];
        for r in REGS {
            if !self.used_gprs_contains(r) {
                return Some(*r);
            }
        }
        None
    }
    fn pick_temp_gpr(&self) -> Option<Self::GPR> {
        use GPR::*;
        static REGS: &[GPR] = &[X17, X16, X15, X14, X13, X12];
        for r in REGS {
            if !self.used_gprs_contains(r) {
                return Some(*r);
            }
        }
        None
    }
    fn get_used_gprs(&self) -> Vec<Self::GPR> {
        todo!()
    }
    fn get_used_simd(&self) -> Vec<Self::SIMD> {
        todo!()
    }
    fn acquire_temp_gpr(&mut self) -> Option<Self::GPR> {
        todo!()
    }
    fn release_gpr(&mut self, gpr: Self::GPR) {
        assert!(self.used_gprs_remove(&gpr));
    }
    fn reserve_unused_temp_gpr(&mut self, gpr: Self::GPR) -> Self::GPR {
        todo!()
    }
    fn reserve_gpr(&mut self, gpr: Self::GPR) {
        self.used_gprs_insert(gpr);
    }
    fn push_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<usize, CompileError> {
        todo!()
    }
    fn pop_used_gpr(&mut self, grps: &[Self::GPR]) -> Result<(), CompileError> {
        todo!()
    }
    fn pick_simd(&self) -> Option<Self::SIMD> {
        todo!()
    }
    fn pick_temp_simd(&self) -> Option<Self::SIMD> {
        todo!()
    }
    fn acquire_temp_simd(&mut self) -> Option<Self::SIMD> {
        todo!()
    }
    fn reserve_simd(&mut self, simd: Self::SIMD) {
        todo!()
    }
    fn release_simd(&mut self, simd: Self::SIMD) {
        todo!()
    }
    fn push_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<usize, CompileError> {
        todo!()
    }
    fn pop_used_simd(&mut self, simds: &[Self::SIMD]) -> Result<(), CompileError> {
        todo!()
    }
    fn round_stack_adjust(&self, value: usize) -> usize {
        // RiscV calling convention states that the stack pointer
        // must be kept 16-byte aligned.

        if value & 0xf != 0 {
            ((value >> 4) + 1) << 4
        } else {
            value
        }
    }
    fn set_srcloc(&mut self, offset: u32) {
        self.src_loc = offset;
    }
    fn mark_address_range_with_trap_code(&mut self, code: TrapCode, begin: usize, end: usize) {
        todo!()
    }
    fn mark_address_with_trap_code(&mut self, code: TrapCode) {
        todo!()
    }
    fn mark_instruction_with_trap_code(&mut self, code: TrapCode) -> usize {
        todo!()
    }
    fn mark_instruction_address_end(&mut self, begin: usize) {
        todo!()
    }
    fn insert_stackoverflow(&mut self) {
        // TODO(challenge)
    }
    fn collect_trap_information(&self) -> Vec<TrapInformation> {
        self.trap_table
            .offset_to_code
            .clone()
            .into_iter()
            .map(|(offset, code)| TrapInformation {
                code_offset: offset as u32,
                trap_code: code,
            })
            .collect()
    }
    fn instructions_address_map(&self) -> Vec<InstructionAddressMap> {
        self.instructions_address_map.clone()
    }
    fn local_on_stack(&mut self, stack_offset: i32) -> Location {
        todo!()
    }
    fn adjust_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn restore_stack(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        todo!()
    }
    fn pop_stack_locals(&mut self, delta_stack_offset: u32) -> Result<(), CompileError> {
        todo!()
    }
    fn zero_location(&mut self, size: Size, location: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn local_pointer(&self) -> Self::GPR {
        GPR::X8
    }
    fn move_location_for_native(
        &mut self,
        size: Size,
        loc: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn is_local_on_stack(&self, idx: usize) -> bool {
        idx > 8
    }
    fn get_local_location(&self, idx: usize, callee_saved_regs_size: usize) -> Location {
        // TODO(challenge): double-check this because I am not sure its right.
        // Use callee-saved registers for the first locals.
        match idx {
            0 => Location::GPR(GPR::X18),
            1 => Location::GPR(GPR::X19),
            2 => Location::GPR(GPR::X20),
            3 => Location::GPR(GPR::X21),
            4 => Location::GPR(GPR::X22),
            5 => Location::GPR(GPR::X23),
            6 => Location::GPR(GPR::X24),
            7 => Location::GPR(GPR::X25),
            8 => Location::GPR(GPR::X26),
            _ => Location::Memory(GPR::X8, -(((idx - 8) * 8 + callee_saved_regs_size) as i32)),
        }
    }
    fn move_local(&mut self, _stack_offset: i32, _location: Location) -> Result<(), CompileError> {
        // TODO(challenge): atually implement this.
        Ok(())
    }
    fn list_to_save(&self, _calling_convention: CallingConvention) -> Vec<Location> {
        // TODO(challenge): check if there are registers that must be saved.
        // TODO(challenge): also, is this registers to save as a callee?
        vec![]
    }
    fn get_param_location(
        &self,
        idx: usize,
        sz: Size,
        stack_offset: &mut usize,
        calling_convention: CallingConvention,
    ) -> Location {
        todo!()
    }
    fn get_call_param_location(
        &self,
        idx: usize,
        _sz: Size,
        stack_offset: &mut usize,
        // TODO(challenge): calling conventions are ignored at this stage.
        _calling_convention: CallingConvention,
    ) -> Location {
        match idx {
            0 => Location::GPR(GPR::X12),
            1 => Location::GPR(GPR::X13),
            2 => Location::GPR(GPR::X14),
            3 => Location::GPR(GPR::X15),
            4 => Location::GPR(GPR::X16),
            5 => Location::GPR(GPR::X17),
            _ => {
                let loc = Location::Memory(GPR::X8, 16 * 2 + *stack_offset as i32);
                *stack_offset += 8;
                loc
            }
        }
    }
    
    fn get_simple_param_location(
        &self,
        idx: usize,
        _calling_convention: CallingConvention,
    ) -> Location {
         match idx {
            0 => Location::GPR(GPR::X12),
            1 => Location::GPR(GPR::X13),
            2 => Location::GPR(GPR::X14),
            3 => Location::GPR(GPR::X15),
            4 => Location::GPR(GPR::X16),
            5 => Location::GPR(GPR::X17),
            _ =>  Location::Memory(GPR::X8, (16 * 2 + (idx - 6) * 8) as i32),
        }
    }

    fn move_location(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        // TODO(challenge): Actually implement this operation
        Ok(())
    }
    fn move_location_extend(
        &mut self,
        size_val: Size,
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) -> Result<(), CompileError> {
        // TODO(challenge): Actually implement this operation
        Ok(())
    }
    fn load_address(
        &mut self,
        size: Size,
        gpr: Location,
        mem: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn init_stack_loc(
        &mut self,
        init_stack_loc_cnt: u64,
        last_stack_loc: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn restore_saved_area(&mut self, saved_area_offset: i32) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn pop_location(&mut self, location: Location) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn new_machine_state(&self) -> MachineState {
        new_machine_state()
    }
    fn assembler_finalize(mut self) -> Result<Vec<u8>, CompileError> {
        dynasm::dynasm!(
            &mut self.assembler
            ; .arch riscv64
            ; addi a1, a1, 1
            ; addi a2, a2, 2
            ; add  a0, a1, a2
            ; ret
        );

        self.assembler.finalize().map_err(|err| CompileError::Codegen(err.to_string()))
    }
    fn get_offset(&self) -> Offset {
        self.assembler.get_offset()
    }
    fn finalize_function(&mut self) -> Result<(), CompileError> {
        // TOOD(challenge)
        Ok(())
    }
    fn emit_function_prolog(&mut self) -> Result<(), CompileError> {
        // TODO(challenge): actually implement function prologs.
        Ok(())
    }
    fn emit_function_epilog(&mut self) -> Result<(), CompileError> {
        // TODO(challenge): actually implement function epilog.
        Ok(())
    }
    fn emit_function_return_value(
        &mut self,
        ty: WpType,
        cannonicalize: bool,
        loc: Location,
    ) -> Result<(), CompileError> {
        // TODO(challenge): this should be easy since its just moving `loc` to the return address x10-x11.
        // TODO(challenge): for this challenge we should not worry about the value not fitting in a int addrees.
        Ok(())
    }
    fn emit_function_return_float(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn arch_supports_canonicalize_nan(&self) -> bool {
        todo!()
    }
    fn canonicalize_nan(
        &mut self,
        sz: Size,
        input: Location,
        output: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_illegal_op(&mut self, trp: TrapCode) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn get_label(&mut self) -> Label {
        self.assembler.new_dynamic_label()
    }
    fn emit_label(&mut self, label: Label) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn get_grp_for_call(&self) -> Self::GPR {
        todo!()
    }
    fn emit_call_register(&mut self, register: Self::GPR) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_label(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn arch_requires_indirect_call_trampoline(&self) -> bool {
        todo!()
    }
    fn arch_emit_indirect_call_with_trampoline(
        &mut self,
        location: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_location(&mut self, location: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn get_gpr_for_ret(&self) -> Self::GPR {
        todo!()
    }
    fn get_simd_for_ret(&self) -> Self::SIMD {
        todo!()
    }
    fn emit_debug_breakpoint(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn location_address(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_and(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_xor(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_or(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_add(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_sub(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
        flags: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_neg(
        &mut self,
        size_val: Size, // size of src
        signed: bool,
        source: Location,
        size_op: Size,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_cmp(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn location_test(
        &mut self,
        size: Size,
        source: Location,
        dest: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_unconditionnal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_equal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_different(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_above(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_aboveequal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_belowequal(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn jmp_on_overflow(&mut self, label: Label) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_jmp_to_jumptable(&mut self, label: Label, cond: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn align_for_loop(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_ret(&mut self) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn emit_push(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_pop(&mut self, size: Size, loc: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_relaxed_mov(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn emit_relaxed_cmp(
        &mut self,
        sz: Size,
        src: Location,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_memory_fence(&mut self) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_relaxed_zero_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_relaxed_sign_extension(
        &mut self,
        sz_src: Size,
        src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_imul_imm32(
        &mut self,
        size: Size,
        imm32: u32,
        gpr: Self::GPR,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_add32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        // TODO(challenge)
        Ok(())
    }
    fn emit_binop_sub32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_mul32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_udiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_sdiv32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_urem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_srem32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_and32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_or32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_xor32(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_load_8s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_load_16s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_save_8(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_save_16(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_save_8(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_save_16(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_add(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_add_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_add_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_sub(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_sub_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_sub_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_and(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_and_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_and_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_or(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_or_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_or_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_xor(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_xor_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_xor_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_xchg(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_xchg_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_xchg_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_cmpxchg(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_cmpxchg_8u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i32_atomic_cmpxchg_16u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_call_with_reloc(
        &mut self,
        calling_convention: CallingConvention,
        reloc_target: RelocationTarget,
    ) -> Result<Vec<Relocation>, CompileError> {
        todo!()
    }
    fn emit_binop_add64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_sub64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_mul64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_udiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_sdiv64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_urem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_srem64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
        integer_division_by_zero: Label,
        integer_overflow: Label,
    ) -> Result<usize, CompileError> {
        todo!()
    }
    fn emit_binop_and64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_or64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_binop_xor64(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_ge_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_gt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_le_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_lt_s(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_ge_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_gt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_le_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_lt_u(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_clz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_ctz(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_popcnt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_shl(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_shr(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_sar(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_rol(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_ror(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load_8s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load_32u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load_32s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_load_16s(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_load_8u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_load_16u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_load_32u(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_save_8(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_save_16(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_save_32(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_save_8(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_save_16(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_save_32(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_add(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_add_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_add_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_add_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_sub(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_sub_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_sub_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_sub_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_and(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_and_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_and_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_and_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_or(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_or_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_or_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_or_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xor(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xor_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xor_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xor_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xchg(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xchg_8u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xchg_16u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_xchg_32u(
        &mut self,
        loc: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_cmpxchg(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_cmpxchg_8u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_cmpxchg_16u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn i64_atomic_cmpxchg_32u(
        &mut self,
        new: Location,
        cmp: Location,
        target: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        canonicalize: bool,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_load(
        &mut self,
        addr: Location,
        memarg: &MemArg,
        ret: Location,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_save(
        &mut self,
        value: Location,
        memarg: &MemArg,
        addr: Location,
        canonicalize: bool,
        need_check: bool,
        imported_memories: bool,
        offset: i32,
        heap_access_oob: Label,
        unaligned_atomic: Label,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f64_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f64_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f32_i64(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f32_i32(
        &mut self,
        loc: Location,
        signed: bool,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i64_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i32_f64(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i64_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_i32_f32(
        &mut self,
        loc: Location,
        ret: Location,
        signed: bool,
        sat: bool,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f64_f32(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn convert_f32_f64(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_i64_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f64_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_neg(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_abs(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn emit_i32_copysign(&mut self, tmp1: Self::GPR, tmp2: Self::GPR) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_sqrt(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_trunc(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_ceil(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_floor(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_nearest(&mut self, loc: Location, ret: Location) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_ge(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_gt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_le(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_lt(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_ne(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_cmp_eq(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_min(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_max(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_add(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_sub(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_mul(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn f32_div(
        &mut self,
        loc_a: Location,
        loc_b: Location,
        ret: Location,
    ) -> Result<(), CompileError> {
        todo!()
    }
    fn gen_std_trampoline(
        &self,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        gen_std_trampoline_riscv64(sig, calling_convention)
    }
    fn gen_std_dynamic_import_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<FunctionBody, CompileError> {
        todo!()
    }
    fn gen_import_call_trampoline(
        &self,
        vmoffsets: &VMOffsets,
        index: FunctionIndex,
        sig: &FunctionType,
        calling_convention: CallingConvention,
    ) -> Result<CustomSection, CompileError> {
        todo!()
    }
    fn gen_dwarf_unwind_info(&mut self, code_len: usize) -> Option<UnwindInstructions> {
        todo!()
    }
    fn gen_windows_unwind_info(&mut self, code_len: usize) -> Option<Vec<u8>> {
        todo!()
    }
}
