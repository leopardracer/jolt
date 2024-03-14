// Handwritten circuit 

use ff::PrimeField; 
use spartan2::r1cs::R1CSShape; 

/* Compiler Variables */
const C: usize = 4; 
const N_FLAGS: usize = 17; 
const W: usize = 32;
const LOG_M: usize = 16; 
const PROG_START_ADDR: usize = 2147483664;
const RAM_START_ADDRESS: usize = 0x80000000; 
const MEMORY_ADDRESS_OFFSET: usize = 0x80000000 - 0x20; 
// "memreg ops per step" 
const MOPS: usize = 7;
/* End of Compiler Variables */

const L_CHUNK: usize = LOG_M/2;  
const STEP_NUM_IDX: usize = 0; 
const PC_IDX: usize = 1; 

const ALL_ONES: i64 = 0xffffffff;

const ONE: (usize, i64) = (0, 1);

const STATE_LENGTH: usize = 2;
const INPUTS: &[(&str, usize)] = &[
    ("CONSTANT", 1), 
    ("output_state", STATE_LENGTH),
    ("input_state", STATE_LENGTH),
    ("prog_a_rw", 1),
    ("prog_v_rw", 6),
    ("memreg_a_rw", 7),
    ("memreg_v_reads", 7),
    ("memreg_v_writes", 7),
    ("chunks_x", C),
    ("chunks_y", C),
    ("chunks_query", C),
    ("lookup_output", 1),
    ("op_flags", N_FLAGS),
];

fn GET_TOTAL_LEN() -> usize {
    INPUTS.iter().map(|(_, value)| value).sum()
}

fn GET_INDEX(name: &str, offset: usize) -> Option<usize> {
    let mut total = 0;
    for &(input_name, value) in INPUTS {
        if input_name == name {
            return Some(total + offset);
        }
        total += value;
    }
    None
}

#[derive(Debug)]
pub struct R1CSBuilder<F: PrimeField> {
    pub A: Vec<(usize, usize, i64)>,
    pub B: Vec<(usize, usize, i64)>,
    pub C: Vec<(usize, usize, i64)>,
    pub num_constraints: usize,
    pub num_variables: usize,
    pub num_inputs: usize, 
    pub num_aux: usize, 
    pub z: Option<Vec<F>>,
}

fn subtract_vectors(x: Vec<(usize, i64)>, mut y: Vec<(usize, i64)>) -> Vec<(usize, i64)> {
    y.extend(x.into_iter().map(|(idx, coeff)| (idx, -coeff)));
    y
}

// big-endian
fn combine_chunks_vec(start_idx: usize, L: usize, N: usize) -> Vec<(usize, i64)> {
    let mut result = vec![];
    for i in 0..N {
        result.push((start_idx + i, 1 << ((N-1-i)*L)));
    }
    result
}

fn i64_to_f<F: PrimeField>(num: i64) -> F {
    if num < 0 {
        F::ZERO - F::from(num.abs() as u64)
    } else {
        F::from(num as u64)
    }
}

impl<F: PrimeField> R1CSBuilder<F> {
    fn new_constraint(&mut self, a: Vec<(usize, i64)>, b: Vec<(usize, i64)>, c: Vec<(usize, i64)>) {
        let row: usize = self.num_constraints; 
        let prepend_row = |vec: Vec<(usize, i64)>| {
            vec.into_iter().map(|(idx, val)| (row, idx, val)).collect::<Vec<(usize, usize, i64)>>()
        };
        self.A.extend(prepend_row(a).into_iter()); 
        self.B.extend(prepend_row(b).into_iter()); 
        self.C.extend(prepend_row(c).into_iter()); 
        self.num_constraints += 1;
    }

    fn get_val_from_lc(&self, lc: &[(usize, i64)]) -> F {
        lc.iter().map(|(idx, coeff)| self.z.as_ref().unwrap()[*idx] * i64_to_f::<F>(*coeff)).sum()
    }

    fn assign_aux(&mut self) -> usize {
        if self.z.is_some() {
            self.z.as_mut().unwrap().push(F::ZERO)
        }
        let idx = self.num_variables; 
        self.num_aux += 1;
        self.num_variables += 1;
        idx
    }   

    fn constr_abc(&mut self, a: Vec<(usize, i64)>, b: Vec<(usize, i64)>, c: Vec<(usize, i64)>) {
        self.new_constraint(a, b, c); 
    }

    // Combines the L-bit wire values of [start_idx, ..., start_idx + N - 1] into a single value 
    // and constraints it to be equal to the wire value at result_idx.
    // The combination is big-endian with the most significant bit at start_idx.
    fn combine_constraint(&mut self, start_idx: usize, L: usize, N: usize, result_idx: usize) {
        let mut constraint_A = vec![];
        let mut constraint_B = vec![];
        let mut constraint_C = vec![];

        for i in 0..N {
            constraint_A.push((start_idx + i, 1 << ((N-1-i)*L)));
        }
        constraint_B.push(ONE); 
        constraint_C.push((result_idx, 1));

        self.new_constraint(constraint_A, constraint_B, constraint_C); 
    }

    fn combine_le(&mut self, start_idx: usize, L: usize, N: usize) -> usize {
        let result_idx = Self::assign_aux(self);

        let mut constraint_A = vec![];
        let mut constraint_B = vec![];
        let mut constraint_C = vec![];

        for i in 0..N {
            constraint_A.push((start_idx + i, 1 << (i*L)));
        }
        constraint_B.push(ONE); 
        constraint_C.push((result_idx, 1));

        if self.z.is_some() {
            let result_value = self.get_val_from_lc(&constraint_A);
            self.z.as_mut().unwrap()[result_idx] = result_value;
        }

        self.new_constraint(constraint_A, constraint_B, constraint_C); 
        result_idx
    }

    fn combine_be(&mut self, start_idx: usize, L: usize, N: usize) -> usize {
        let result_idx = Self::assign_aux(self);

        let mut constraint_A = vec![];
        let mut constraint_B = vec![];
        let mut constraint_C = vec![];

        for i in 0..N {
            constraint_A.push((start_idx + i, 1 << ((N-1-i)*L)));
        }
        constraint_B.push(ONE); 
        constraint_C.push((result_idx, 1));

        if self.z.is_some() {
            let result_value = self.get_val_from_lc(&constraint_A);
            self.z.as_mut().unwrap()[result_idx] = result_value;
        }

        self.new_constraint(constraint_A, constraint_B, constraint_C); 
        result_idx
    }

    fn multiply(&mut self, x: Vec<(usize, i64)>, y: Vec<(usize, i64)>) -> usize {
        let xy_idx = Self::assign_aux(self); 
        if self.z.is_some() {
            let x = self.get_val_from_lc(&x);
            let y = self.get_val_from_lc(&y);
            self.z.as_mut().unwrap()[xy_idx] = x * y;
        }

        R1CSBuilder::constr_abc(self, 
            x, 
            y, 
            vec![(xy_idx, 1)], 
        ); 

        xy_idx 
    }

    fn if_else(&mut self, choice: Vec<(usize, i64)>, x: Vec<(usize, i64)>, y: Vec<(usize, i64)>) -> usize {
        let z_idx = Self::assign_aux(self);
        if self.z.is_some() {
            let choice_val = self.get_val_from_lc(&choice);
            let x_val = self.get_val_from_lc(&x);
            let y_val = self.get_val_from_lc(&y);
            let z_val = if choice_val == F::ZERO { x_val } else { y_val };
            self.z.as_mut().unwrap()[z_idx] = z_val;
        }

        let y_minus_x = subtract_vectors(y, x.clone()); 
        let z_minus_x = subtract_vectors(vec![(z_idx, 1)], x);

        self.new_constraint(choice, y_minus_x, z_minus_x); 
        z_idx 
    }

    fn if_else_simple(&mut self, choice: usize, x: usize, y: usize) -> usize {
        Self::if_else(self, vec![(choice, 1)], vec![(x, 1)], vec![(y, 1)])  
    }

    // The left side is an lc but the right is a single index
    fn eq_simple(&mut self, left_idx: usize, right_idx: usize) {
        let mut constraint_A = vec![];
        let mut constraint_B = vec![];
        let mut constraint_C = vec![];

        constraint_A.push((left_idx, 1));
        constraint_B.push(ONE); 
        constraint_C.push((right_idx, 1));

        self.new_constraint(constraint_A, constraint_B, constraint_C); 
    }

    // The left side is an lc but the right is a single index
    fn eq(&mut self, left: Vec<(usize, i64)>, right_idx: usize, assign: bool) {
        let mut constraint_A = vec![];
        let mut constraint_B = vec![];
        let mut constraint_C = vec![];

        constraint_A = left; 
        constraint_B.push(ONE); 
        constraint_C.push((right_idx, 1));

        if assign && self.z.is_some() {
            self.z.as_mut().unwrap()[right_idx] = self.get_val_from_lc(&constraint_A);
        }

        self.new_constraint(constraint_A, constraint_B, constraint_C); 
    }

    fn constr_prod_0(&mut self, x: Vec<(usize, i64)>, y: Vec<(usize, i64)>, z: Vec<(usize, i64)>) {
        let xy_idx = Self::assign_aux(self); 

        if self.z.is_some() {
            let x_val = self.get_val_from_lc(&x);
            let y_val = self.get_val_from_lc(&y);
            self.z.as_mut().unwrap()[xy_idx] = x_val * y_val;
        }

        R1CSBuilder::constr_abc(self, 
            x, 
            y, 
            vec![(xy_idx, 1)], 
        ); 

        R1CSBuilder::constr_abc(self, 
            vec![(xy_idx, 1)], 
            z, 
            vec![], 
        ); 
    }

    pub fn get_matrices(inputs: Option<Vec<F>>) -> Option<Self> {
        // Append the constant 1 to the inputs 
        let inputs_with_const_output = inputs.map(|mut vec| {
            vec.insert(0, F::ZERO);
            vec.insert(0, F::ZERO);
            vec.insert(0, F::ONE); // constant
            vec
        });

        let mut instance = R1CSBuilder {
            A: vec![],
            B: vec![],
            C: vec![],
            num_constraints: 0,
            num_variables: GET_TOTAL_LEN(), // includes ("constant", 1) and ("output_state", ..)
            num_inputs: 0, // technically inputs are also aux, so keep this 0
            num_aux: GET_TOTAL_LEN()-1, // dont' include the constant  
            z: inputs_with_const_output, 
        };

        // Parse the input indices 
        let opcode = GET_INDEX("prog_v_rw", 0)?;
        let rs1 = GET_INDEX("prog_v_rw", 1)?;
        let rs2 = GET_INDEX("prog_v_rw", 2)?;
        let rd = GET_INDEX("prog_v_rw", 3)?;
        let immediate_before_processing = GET_INDEX("prog_v_rw", 4)?;
        let op_flags_packed = GET_INDEX("prog_v_rw", 5)?;

        let is_load_instr: usize = GET_INDEX("op_flags", 2)?;
        let is_store_instr: usize = GET_INDEX("op_flags", 3)?;
        let is_jump_instr: usize = GET_INDEX("op_flags", 4)?;
        let is_branch_instr: usize = GET_INDEX("op_flags", 5)?;
        let if_update_rd_with_lookup_output: usize = GET_INDEX("op_flags", 6)?;
        let is_add_instr: usize = GET_INDEX("op_flags", 7)?;
        let is_sub_instr: usize = GET_INDEX("op_flags", 8)?;
        let is_mul_instr: usize = GET_INDEX("op_flags", 9)?;
        let is_advice_instr: usize = GET_INDEX("op_flags", 10)?;
        let is_assert_false_instr: usize = GET_INDEX("op_flags", 11)?;
        let is_assert_true_instr: usize = GET_INDEX("op_flags", 12)?;
        let sign_imm_flag: usize = GET_INDEX("op_flags", 13)?;
        let is_concat: usize = GET_INDEX("op_flags", 14)?;
        let is_lui_auipc: usize = GET_INDEX("op_flags", 15)?;
        let is_shift: usize = GET_INDEX("op_flags", 16)?;

        let PC = GET_INDEX("input_state", PC_IDX)?; 

        // Constraint 1: relation between PC and prog_a_rw
        // TODO(arasuarun): this should be done after fixing the padding issue for prog_a_rw

        // Combine flag_bits and check that they equal op_flags_packed. 
        R1CSBuilder::combine_constraint(&mut instance, GET_INDEX("op_flags", 0)?, 1, N_FLAGS, op_flags_packed);

        // Constriant: signal immediate <== if_else()([is_lui_auipc, immediate_before_processing, immediate_before_processing * (2**12)]);
        let immediate: usize = R1CSBuilder::if_else(&mut instance, vec![(is_lui_auipc, 1)], vec![(immediate_before_processing, 1)], vec![(immediate_before_processing, 1<<12)]); 

        // Constraint: rs1 === memreg_a_rw[0];
        R1CSBuilder::eq_simple(&mut instance, rs1, GET_INDEX("memreg_a_rw", 0)?); 
        // Constraint: rs2 === memreg_a_rw[1];
        R1CSBuilder::eq_simple(&mut instance, rs2, GET_INDEX("memreg_a_rw", 1)?);
        // Constraint: rd === memreg_a_rw[2];
        R1CSBuilder::eq_simple(&mut instance, rd, GET_INDEX("memreg_a_rw", 2)?);
        // Constraint: memreg_v_reads[0] === memreg_v_writes[0];
        R1CSBuilder::eq_simple(&mut instance, GET_INDEX("memreg_v_reads", 0)?, GET_INDEX("memreg_v_writes", 0)?); 
        // Constraint: memreg_v_reads[1] === memreg_v_writes[1];
        R1CSBuilder::eq_simple(&mut instance, GET_INDEX("memreg_v_reads", 1)?, GET_INDEX("memreg_v_writes", 1)?);

        let rs1_val = GET_INDEX("memreg_v_reads", 0)?;
        let rs2_val = GET_INDEX("memreg_v_reads", 1)?;

        /*
            signal x <== if_else()([op_flags[0], rs1_val, PC]); // TODO: change this for virtual instructions
            signal _y <== if_else()([op_flags[1], rs2_val, immediate]);
            signal y <== if_else()([1-is_advice_instr, lookup_output, _y]);
         */
        let x = R1CSBuilder::if_else_simple(&mut instance, GET_INDEX("op_flags", 0)?, rs1_val, PC);
        let _y = R1CSBuilder::if_else_simple(&mut instance, GET_INDEX("op_flags", 1)?, rs2_val, immediate);
        let y = R1CSBuilder::if_else_simple(&mut instance, is_advice_instr, _y, GET_INDEX("lookup_output", 0)?); 

        /*
            signal mem_v_bytes[MOPS()-3] <== subarray(3, MOPS()-3, MOPS())(memreg_v_writes);
            signal load_or_store_value <== combine_chunks_le(MOPS()-3, 8)(mem_v_bytes); 
        */
        let load_or_store_value = R1CSBuilder::combine_le(&mut instance, GET_INDEX("memreg_v_writes", MOPS-3)?, 8, MOPS); 

        /* 
            signal immediate_signed <== if_else()([sign_imm_flag, immediate, -ALL_ONES() + immediate - 1]);
            (is_load_instr + is_store_instr) * ((rs1_val + immediate_signed) - (memreg_a_rw[3] + MEMORY_ADDRESS_OFFSET())) === 0;
        */
        let immediate_signed = R1CSBuilder::if_else(&mut instance, vec![(sign_imm_flag, 1)], vec![(immediate, 1)], vec![(immediate, 1), (0, -ALL_ONES - 1)]);
        R1CSBuilder::constr_abc(&mut instance, vec![(is_load_instr, 1), (is_store_instr, 1)], vec![(rs1_val, 1), (immediate_signed, 1), (GET_INDEX("memreg_a_rw", 3)?, -1), (0, -1 * MEMORY_ADDRESS_OFFSET as i64)], vec![]); 

        /*
            for (var i=1; i<MOPS()-3; i++) {
                // the first three are rs1, rs2, rd so memory starts are index 3
                (memreg_a_rw[3+i] - (memreg_a_rw[3] + i)) *  memreg_a_rw[3+i] === 0; 
            }
        */
        for i in 1..MOPS-3 {
            R1CSBuilder::constr_abc(&mut instance, vec![(GET_INDEX("memreg_a_rw", 3+i)?, 1), (GET_INDEX("memreg_a_rw", 3)?, -1), (0, i as i64 * -1)], vec![(GET_INDEX("memreg_a_rw", 3+i)?, 1)], vec![]);
        }

        /*
            for (var i=0; i<MOPS()-3; i++) {
                (memreg_v_reads[3+i] - memreg_v_writes[3+i]) * is_load_instr === 0;
            }
        */
        for i in 0..MOPS-3 {
            R1CSBuilder::constr_abc(&mut instance, vec![(GET_INDEX("memreg_v_reads", 3+i)?, 1)], vec![(GET_INDEX("memreg_v_writes", 3+i)?, -1)], vec![(is_load_instr, 1)]);
        }

        // is_store_instr * (load_or_store_value - rs2_val) === 0;
        R1CSBuilder::constr_abc(&mut instance, vec![(is_store_instr, 1)], vec![(load_or_store_value, 1), (rs2_val, -1)], vec![]);


        /* Create the lookup query 
        - First, obtain combined_z_chunks (which should be the query)
        - Verify that the query is structured correctly, based on the instruction.
        - Then verify that the chunks of x, y, z are correct. 

        Constraints to check correctness of chunks_query 
            If NOT a concat query: chunks_query === chunks_z 
            If its a concat query: then chunks_query === zip(chunks_x, chunks_y)
        For concat, queries the tests are done later. 

            signal combined_z_chunks <== combine_chunks(C(), LOG_M())(chunks_query);
            is_add_instr * (combined_z_chunks - (x + y)) === 0; 
            is_sub_instr * (combined_z_chunks - (x + (ALL_ONES() - y + 1))) === 0; 

            // This creates a big aux witness value only for mul instructions. 
            signal is_mul_x <== is_mul_instr * x; 
            signal is_mul_xy <== is_mul_instr * y;
            is_mul_instr * (combined_z_chunks - is_mul_xy) === 0;
        */

        let combined_z_chunks = R1CSBuilder::combine_be(&mut instance, GET_INDEX("chunks_query", 0)?, LOG_M, C);
        R1CSBuilder::constr_abc(&mut instance,
            vec![(is_add_instr, 1)], 
            vec![(combined_z_chunks, 1), (x, -1), (y, -1)],
            vec![]
        ); 
        R1CSBuilder::constr_abc(&mut instance,
            vec![(is_sub_instr, 1)], 
            vec![(combined_z_chunks, 1), (x, -1), (y, 1), (0, -1 * (ALL_ONES + 1))],
            vec![]
        ); 

        let is_mul_x = R1CSBuilder::multiply(&mut instance, vec![(is_mul_instr, 1)], vec![(x, 1)]);
        let is_mul_xy = R1CSBuilder::multiply(&mut instance, vec![(is_mul_x, 1)], vec![(y, 1)]);
        R1CSBuilder::constr_abc(&mut instance,
            vec![(is_mul_instr, 1)], 
            vec![(combined_z_chunks, 1), (is_mul_xy, -1)],
            vec![]
        );

    /* Verify the chunks of x and y for concat instructions. 
        signal combined_x_chunks <== combine_chunks(C(), L_CHUNK())(chunks_x);
        (combined_x_chunks - x) * is_concat === 0; 

        Note that a wire value need not be assigned for combined x_chunks. 
        Repeat for y. 
    */
        R1CSBuilder::constr_abc(
            &mut instance,  
            [
                combine_chunks_vec(GET_INDEX("chunks_x", 0)?, L_CHUNK, C), 
                vec![(x, -1)]
            ].concat(),
            vec![(is_concat, 1)],
            vec![], 
        ); 
        R1CSBuilder::constr_abc(
            &mut instance,  
            [
                combine_chunks_vec(GET_INDEX("chunks_y", 0)?, L_CHUNK, C),
                vec![(y, -1)]
            ].concat(),
            vec![(is_concat, 1)],
            vec![], 
        ); 

        /* Concat query construction: 

            signal chunk_y_used[C()]; 
            for (var i=0; i<C(); i++) {
                chunk_y_used[i] <== if_else()([is_shift, chunks_y[i], chunks_y[C()-1]]);
                (chunks_query[i] - (chunk_y_used[i] + chunks_x[i] * 2**L_CHUNK())) * is_concat === 0;
            }  
        */
        for i in 0..C {
            let chunk_y_used_i = R1CSBuilder::if_else_simple(&mut instance, is_shift, GET_INDEX("chunks_y", i)?, GET_INDEX("chunks_y", C-1)?);
            R1CSBuilder::constr_abc(
                &mut instance,  
                vec![(GET_INDEX("chunks_query", i)?, 1), (chunk_y_used_i, -1), (GET_INDEX("chunks_x", i)?, (1<<L_CHUNK)*-1)],
                vec![(is_concat, 1)],
                vec![], 
            ); 
        }

        // TODO: handle case when C() doesn't divide W() 
        // var idx_ms_chunk = C()-1;
        // (chunks_query[idx_ms_chunk] - (chunks_x[idx_ms_chunk] + chunks_y[idx_ms_chunk] * 2**(L_MS_CHUNK()))) * is_concat === 0;

        /* For assert instructions 
        is_assert_false_instr * (1-lookup_output) === 0;
        is_assert_true_instr * lookup_output === 0;
        */
        R1CSBuilder::constr_abc(&mut instance, vec![(is_assert_false_instr, 1)], vec![(GET_INDEX("lookup_output", 0)?, -1), (0, 1)], vec![]); 
        R1CSBuilder::constr_abc(&mut instance, vec![(is_assert_true_instr, 1)], vec![(GET_INDEX("lookup_output", 0)?, 1)], vec![]); 

        /* Constraints for storing value in register rd.
            // lui doesn't need a lookup and simply requires the lookup_output to be set to immediate 
            // so it can be stored in the destination register. 

            signal rd_val <== memreg_v_writes[2];
            is_load_instr * (rd_val - load_or_store_value) === 0;

            component rd_test_lookup = prodZeroTest(3);
            rd_test_lookup.in <== [rd, if_update_rd_with_lookup_output, (rd_val - lookup_output)]; 
            component rd_test_jump = prodZeroTest(3); 
            rd_test_jump.in <== [rd, is_jump_instr, (rd_val - (prog_a_rw + 4))]; 
        */
        let rd_val = GET_INDEX("memreg_v_writes", 2)?; 
        R1CSBuilder::constr_abc(&mut instance, vec![(is_load_instr, 1)], vec![(rd_val, 1), (load_or_store_value, -1)], vec![]);
        R1CSBuilder::constr_prod_0(
            &mut instance, 
            vec![(rd, 1)], 
            vec![(if_update_rd_with_lookup_output, 1)], 
            vec![(rd_val, 1), (GET_INDEX("lookup_output", 0)?, -1)], 
        );
        R1CSBuilder::constr_prod_0(
            &mut instance, 
            vec![(rd, 1)], 
            vec![(is_jump_instr, 1)], 
            vec![(rd_val, 1), (PC, -1), (0, -4)], 
        ); 
        
        // output_state[STEP_NUM_IDX()] <== input_state[STEP_NUM_IDX()]+1;
        // R1CSBuilder::constr_abc(&mut instance, 
        //     vec![(GET_INDEX("output_state", STEP_NUM_IDX)?, 1)], 
        //     vec![(GET_INDEX("input_state", STEP_NUM_IDX)?, 1), (0, 1)], 
        //     vec![], 
        // );
        R1CSBuilder::eq(&mut instance, 
            vec![(GET_INDEX("input_state", STEP_NUM_IDX)?, 1), (0, 1)], 
            GET_INDEX("output_state", STEP_NUM_IDX)?,
            true, 
        );


        /*  Set next PC 
            signal next_pc_j <== if_else()([
                is_jump_instr,  
                input_state[PC_IDX()] + 4, 
                lookup_output
            ]);
            signal next_pc_j_b <== if_else()([
                is_branch_instr * lookup_output, 
                next_pc_j, 
                input_state[PC_IDX()] + immediate_signed
            ]);
            output_state[PC_IDX()] <== next_pc_j_b;
        */
        let is_branch_times_lookup_output = R1CSBuilder::multiply(&mut instance, vec![(is_branch_instr, 1)], vec![(GET_INDEX("lookup_output", 0)?, 1)]); 

        let next_pc_j = R1CSBuilder::if_else(&mut instance, vec![(is_jump_instr, 1)], vec![(PC, 1), (0, 4)], vec![(GET_INDEX("lookup_output", 0)?, 1)]);

        let next_pc_j_b = R1CSBuilder::if_else(&mut instance, vec![(is_branch_times_lookup_output, 1)], vec![(next_pc_j, 1)], vec![(PC, 1), (immediate_signed, 1)]);

        R1CSBuilder::eq(&mut instance, 
            vec![(next_pc_j_b, 1)], 
            GET_INDEX("output_state", PC_IDX)?, 
            true, 
        );

        Some(instance)
    }

    pub fn convert_to_field(&self) -> (Vec<(usize, usize, F)>, Vec<(usize, usize, F)>, Vec<(usize, usize, F)>) {
        (
            self.A.iter().map(|(row, idx, val)| (*row, *idx, i64_to_f::<F>(*val))).collect(),
            self.B.iter().map(|(row, idx, val)| (*row, *idx, i64_to_f::<F>(*val))).collect(),
            self.C.iter().map(|(row, idx, val)| (*row, *idx, i64_to_f::<F>(*val))).collect(),
        )
    }
}


    #[cfg(test)]
    mod tests {
        use super::*;
        use spartan2::provider::bn256_grumpkin::bn256::Scalar as F;

        #[test]
        fn test_get_matrices() {
            let instance = R1CSBuilder::<F>::get_matrices(None);
            println!("{:?}", instance.unwrap());

        }

        #[test]
        fn test_get_matrices_all_zeros() {
            let instance = R1CSBuilder::<F>::get_matrices(Some(vec![F::from(0); GET_TOTAL_LEN()])).unwrap();
            // println!("{:?}", instance);
            println!("z vector is {:?}", instance.z.unwrap());
            // println!("z[2] vector is {:?}", instance.z.unwrap()[2] * F::from(-1));
        }
    }