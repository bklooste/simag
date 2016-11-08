use std::str;
use std::collections::HashMap;

use lang::parser::*;
use lang::logsent::*;
use agent;

// Predicate types:

#[derive(Debug, Clone)]
pub enum Predicate {
    FreeTerm(FreeTerm),
    GroundedTerm(GroundedTerm),
}

impl<'a> Predicate {
    fn from(a: &'a ArgBorrowed<'a>,
            context: &'a mut Context,
            func_name: &'a Terminal)
            -> Result<Predicate, ParseErrF> {
        match Terminal::from(&a.term, context) {
            Ok(Terminal::FreeTerm(ft)) => {
                let t = FreeTerm::new(ft, a.uval, func_name, None);
                if t.is_err() {
                    return Err(t.unwrap_err());
                }
                Ok(Predicate::FreeTerm(t.unwrap()))
            }
            Ok(Terminal::GroundedTerm(gt)) => {
                let t;
                if context.in_assertion {
                    t = GroundedTerm::new(gt, a.uval, func_name, None, true);
                } else {
                    t = GroundedTerm::new(gt, a.uval, func_name, None, false);
                }
                if t.is_err() {
                    return Err(t.unwrap_err());
                }
                Ok(Predicate::GroundedTerm(t.unwrap()))
            }
            Ok(Terminal::Keyword(kw)) => return Err(ParseErrF::ReservedKW(String::from(kw))),
            Err(err) => Err(err),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroundedTerm {
    term: String,
    value: Option<f32>,
    operator: Option<CompOperator>,
    parent: Terminal,
    dates: Option<Vec<i32>>,
}

impl GroundedTerm {
    fn new(term: String,
           uval: Option<UVal>,
           parent: &Terminal,
           _dates: Option<Vec<i32>>,
           is_assignment: bool)
           -> Result<GroundedTerm, ParseErrF> {
        let val;
        let op;
        if uval.is_some() {
            let uval = uval.unwrap();
            val = match uval.val {
                Number::UnsignedInteger(val) => {
                    if val == 0 || val == 1 {
                        Some(val as f32)
                    } else {
                        return Err(ParseErrF::IUVal(val as f32));
                    }
                }
                Number::UnsignedFloat(val) => {
                    if val >= 0. && val <= 1. {
                        Some(val)
                    } else {
                        return Err(ParseErrF::IUVal(val as f32));
                    }
                }
                Number::SignedFloat(val) => return Err(ParseErrF::IUVal(val as f32)),
                Number::SignedInteger(val) => return Err(ParseErrF::IUVal(val as f32)),
            };
            if is_assignment {
                op = match uval.op {
                    CompOperator::Equal => Some(CompOperator::Equal),
                    _ => return Err(ParseErrF::IUValComp),
                }
            } else {
                op = Some(uval.op);
            }
        } else {
            val = None;
            op = None;
        }
        Ok(GroundedTerm {
            term: term,
            value: val,
            operator: op,
            parent: parent.clone(),
            dates: None,
        })
    }

    fn from_free(free: &FreeTerm,
                 assignments: &HashMap<*const Var, &agent::VarAssignment>)
                 -> Result<GroundedTerm, ()> {
        if let Some(entity) = assignments.get(&free.term) {
            let name = String::from(entity.name);
            Ok(GroundedTerm {
                term: name,
                value: free.value,
                operator: free.operator,
                parent: free.parent.clone(),
                dates: None,
            })
        } else {
            Err(())
        }
    }

    #[inline]
    fn comparable(&self, other: &GroundedTerm) -> bool {
        if self.term != other.get_name() {
            return false;
        }
        if self.parent != other.parent {
            return false;
        }
        true
    }

    pub fn get_name(&self) -> &str {
        &self.term
    }
}

impl ::std::cmp::PartialEq for GroundedTerm {
    fn eq(&self, other: &GroundedTerm) -> bool {
        if self.term != self.term {
            panic!("simag: grounded terms with different names cannot be compared")
        }
        if self.value.is_some() && other.value.is_some() {
            let val_lhs = self.value.as_ref().unwrap();
            let val_rhs = other.value.as_ref().unwrap();
            match self.operator.as_ref().unwrap() {
                &CompOperator::Equal => {
                    if other.operator.as_ref().unwrap().is_equal() {
                        if val_lhs == val_rhs {
                            return true;
                        } else {
                            return false;
                        }
                    } else if other.operator.as_ref().unwrap().is_more() {
                        if val_lhs > val_rhs {
                            return true;
                        } else {
                            return false;
                        }
                    } else {
                        if val_lhs < val_rhs {
                            return true;
                        } else {
                            return false;
                        }
                    }
                }
                &CompOperator::More => {
                    if other.operator.as_ref().unwrap().is_equal() {
                        if val_lhs < val_rhs {
                            return true;
                        } else {
                            return false;
                        }
                    } else {
                        panic!("simag: grounded terms operators in assertments \
                                must be assignments")
                    }
                }
                &CompOperator::Less => {
                    if other.operator.as_ref().unwrap().is_equal() {
                        if val_lhs > val_rhs {
                            return true;
                        } else {
                            return false;
                        }
                    } else {
                        panic!("simag: grounded terms operators in assertments \
                                must be assignments")
                    }
                }
            }
        } else if self.value.is_none() && other.value.is_none() {
            true
        } else {
            panic!("simag: at least one of the two grounded terms does not include a value")
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct GroundedFunc {
    name: String,
    args: [GroundedTerm; 2],
    third: Option<GroundedTerm>,
}

impl GroundedFunc {
    pub fn get_name(&self) -> &str {
        self.name.as_str()
    }

    pub fn comparable_entity(&self, free: &FuncDecl, entity_name: &str, var: Option<*const Var>) -> bool {
        if free.get_name() != self.name {
            return false;
        }
        if var.is_some() {
            let var = var.unwrap();
            if let Some(pos) = free.var_in_pos(var) {
                match pos {
                    0 => {
                        if self.args[0].get_name() == entity_name {
                            return true;
                        }
                    }
                    1 => {
                        if self.args[1].get_name() == entity_name {
                            return true;
                        }
                    }
                    2 => {
                        if let Some(ref term) = self.third {
                            if term.get_name() == entity_name {
                                return true;
                            }
                        }
                    }
                    _ => return false,
                }
            }
        } else {
            if let Some(pos) = free.term_in_pos(entity_name) {
                match pos {
                    0 => {
                        if self.args[0].get_name() == entity_name {
                            return true;
                        }
                    }
                    1 => {
                        if self.args[1].get_name() == entity_name {
                            return true;
                        }
                    }
                    2 => {
                        if let Some(ref term) = self.third {
                            if term.get_name() == entity_name {
                                return true;
                            }
                        }
                    }
                    _ => return false,
                }
            }
        }
        false
    }

    pub fn comparable(&self, other: &GroundedFunc) -> bool {
        if other.get_name() != self.name {
            return false;
        }
        if !self.args[0].comparable(&other.args[0]) {
            return false;
        }
        if !self.args[1].comparable(&other.args[1]) {
            return false;
        }
        if self.third.is_some() && other.third.is_some() {
            if !self.third.as_ref().unwrap().comparable(
                other.third.as_ref().unwrap()
            ) {
                return false;
            }
        } else if self.third.is_none() && other.third.is_none() {
            return true;
        } else {
            return false;
        }
        true
    }

    fn from_free(free: &FuncDecl,
                 assignments: &HashMap<*const Var, &agent::VarAssignment>)
                 -> Result<GroundedFunc, ()> {
        if !free.variant.is_relational() || free.args.as_ref().unwrap().len() < 2 {
            return Err(());
        }
        let name = match free.name {
            Terminal::GroundedTerm(ref name) => name.clone(),
            _ => panic!("simag: expected a grounded terminal, found a free terminal"),
        };
        let mut n_args: [GroundedTerm; 2];
        n_args = unsafe { ::std::mem::uninitialized() };
        let mut third = None;
        for (i, a) in free.args.as_ref().unwrap().iter().enumerate() {
            let n_a = match a {
                &Predicate::FreeTerm(ref free) => {
                    if let Ok(grounded) = GroundedTerm::from_free(free, assignments) {
                        grounded
                    } else {
                        return Err(());
                    }
                }
                &Predicate::GroundedTerm(ref term) => term.clone(),
            };
            if i == 0 {
                n_args[0] = n_a
            } else if i == 3 {
                n_args[1] = n_a
            } else {
                third = Some(n_a)
            }
        }
        Ok(GroundedFunc {
            name: name,
            args: n_args,
            third: third,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FreeTerm {
    term: *const Var,
    value: Option<f32>,
    operator: Option<CompOperator>,
    parent: Terminal,
    dates: Option<Vec<i32>>,
}

impl FreeTerm {
    fn new(term: *const Var,
           uval: Option<UVal>,
           parent: &Terminal,
           _dates: Option<Vec<i32>>)
           -> Result<FreeTerm, ParseErrF> {
        let val;
        let op;
        if uval.is_some() {
            let uval = uval.unwrap();
            val = match uval.val {
                Number::UnsignedInteger(val) => {
                    if val == 0 || val == 1 {
                        Some(val as f32)
                    } else {
                        return Err(ParseErrF::IUVal(val as f32));
                    }
                }
                Number::UnsignedFloat(val) => {
                    if val >= 0. && val <= 1. {
                        Some(val)
                    } else {
                        return Err(ParseErrF::IUVal(val as f32));
                    }
                }
                Number::SignedFloat(val) => return Err(ParseErrF::IUVal(val as f32)),
                Number::SignedInteger(val) => return Err(ParseErrF::IUVal(val as f32)),
            };
            op = Some(uval.op);
        } else {
            val = None;
            op = None;
        }
        Ok(FreeTerm {
            term: term,
            value: val,
            operator: op,
            parent: parent.clone(),
            dates: None,
        })
    }

    fn equal_to_grounded(&self, other: &GroundedTerm) -> bool {
        if self.parent != other.parent {
            panic!("simag: grounded terms from different classes cannot be compared")
        }
        if self.value.is_some() && other.value.is_some() {
            let val_free = self.value.as_ref().unwrap();
            let val_grounded = other.value.as_ref().unwrap();
            match other.operator.as_ref().unwrap() {
                &CompOperator::Equal => {
                    if self.operator.as_ref().unwrap().is_equal() {
                        if val_free == val_grounded {
                            return true;
                        } else {
                            return false;
                        }
                    } else if self.operator.as_ref().unwrap().is_more() {
                        if val_grounded > val_free {
                            return true;
                        } else {
                            return false;
                        }
                    } else {
                        if val_grounded < val_free {
                            return true;
                        } else {
                            return false;
                        }
                    }
                }
                _ => panic!("simag: grounded terms operators in assertments must be assignments"),
            }
        } else {
            panic!("simag: at least one of the two compared terms does not include a value")
        }
    }
}

// Assert types:

#[derive(Debug, Clone)]
pub enum Assert {
    FuncDecl(FuncDecl),
    ClassDecl(ClassDecl),
}

impl Assert {
    #[inline]
    pub fn get_name(&self) -> &str {
        match self {
            &Assert::FuncDecl(ref f) => f.get_name(),
            &Assert::ClassDecl(ref c) => c.get_name(),
        }
    }

    pub fn unwrap_fn(self) -> FuncDecl {
        match self {
            Assert::FuncDecl(f) => f,
            Assert::ClassDecl(_) => {
                panic!("simag: expected a function declaration, found class instead")
            }
        }
    }

    pub fn unwrap_cls(self) -> ClassDecl {
        match self {
            Assert::FuncDecl(_) => {
                panic!("simag: expected a class declaration, found function instead")
            }
            Assert::ClassDecl(c) => c,
        }
    }

    #[inline]
    pub fn equal_to_grounded(&self,
                             agent: &agent::Representation,
                             assignments: &Option<&HashMap<*const Var, &agent::VarAssignment>>)
                             -> Option<bool> {
        match self {
            &Assert::FuncDecl(ref f) => f.equal_to_grounded(agent, assignments),
            &Assert::ClassDecl(ref c) => c.equal_to_grounded(agent, assignments),
        }
    }

    #[inline]
    pub fn is_class(&self) -> bool {
        match self {
            &Assert::FuncDecl(_) => false,
            &Assert::ClassDecl(_) => true,
        }
    }

    #[inline]
    pub fn contains(&self, var: &Var) -> bool {
        match self {
            &Assert::FuncDecl(ref f) => f.contains(var),
            &Assert::ClassDecl(ref c) => c.contains(var),
        }
    }

    #[inline]
    pub fn substitute(&self,
                      agent: &agent::Representation,
                      assignments: &Option<&HashMap<*const Var, &agent::VarAssignment>>) {
        match self {
            &Assert::FuncDecl(ref f) => f.substitute(agent, assignments),
            &Assert::ClassDecl(ref c) => c.substitute(agent, assignments),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    name: Terminal,
    args: Option<Vec<Predicate>>,
    op_args: Option<Vec<OpArg>>,
    variant: FuncVariants,
}

impl<'a> FuncDecl {
    pub fn from(other: &FuncDeclBorrowed<'a>,
                context: &mut Context)
                -> Result<FuncDecl, ParseErrF> {
        let mut variant = other.variant;
        let func_name = match Terminal::from(&other.name, context) {
            Err(ParseErrF::ReservedKW(val)) => {
                if &val == "time_calc" {
                    variant = FuncVariants::TimeCalc;
                    Terminal::Keyword("time_calc")
                } else {
                    return Err(ParseErrF::ReservedKW(val));
                }
            }
            Err(err) => return Err(err),
            Ok(val) => val,
        };
        match variant {
            FuncVariants::TimeCalc => return FuncDecl::decl_timecalc_fn(other, context),
            FuncVariants::Relational => {
                return FuncDecl::decl_relational_fn(other, context, func_name)
            }
            FuncVariants::NonRelational => {
                return FuncDecl::decl_nonrelational_fn(other, context, func_name)
            }
        }
    }

    pub fn into_grounded(self) -> GroundedFunc {
        unimplemented!()
    }

    fn decl_timecalc_fn(other: &FuncDeclBorrowed<'a>,
                        context: &mut Context)
                        -> Result<FuncDecl, ParseErrF> {
        if other.args.is_some() || other.op_args.is_none() {
            return Err(ParseErrF::WrongDef);
        }
        let op_args = match other.op_args {
            Some(ref oargs) => {
                let mut v0 = Vec::with_capacity(oargs.len());
                for e in oargs {
                    let a = match OpArg::from(e, context) {
                        Err(err) => return Err(err),
                        Ok(a) => a,
                    };
                    v0.push(a);
                }
                Some(v0)
            }
            None => return Err(ParseErrF::WrongDef),
        };
        if op_args.as_ref().unwrap().len() != 2 {
            return Err(ParseErrF::WrongDef);
        }
        Ok(FuncDecl {
            name: Terminal::Keyword("time_calc"),
            args: None,
            op_args: op_args,
            variant: FuncVariants::TimeCalc,
        })
    }

    fn decl_relational_fn(other: &FuncDeclBorrowed<'a>,
                          context: &mut Context,
                          name: Terminal)
                          -> Result<FuncDecl, ParseErrF> {
        let args = match other.args {
            Some(ref oargs) => {
                let mut v0 = Vec::with_capacity(oargs.len());
                for a in oargs {
                    let pred = Predicate::from(a, context, &name);
                    if pred.is_err() {
                        return Err(pred.unwrap_err());
                    }
                    v0.push(pred.unwrap());
                }
                Some(v0)
            }
            None => None,
        };
        let op_args = match other.op_args {
            Some(ref oargs) => {
                let mut v0 = Vec::with_capacity(oargs.len());
                for e in oargs {
                    let a = match OpArg::from(e, context) {
                        Err(err) => return Err(err),
                        Ok(a) => a,
                    };
                    v0.push(a);
                }
                Some(v0)
            }
            None => None,
        };
        Ok(FuncDecl {
            name: name,
            args: args,
            op_args: op_args,
            variant: FuncVariants::Relational,
        })
    }

    fn decl_nonrelational_fn(other: &FuncDeclBorrowed<'a>,
                             context: &mut Context,
                             name: Terminal)
                             -> Result<FuncDecl, ParseErrF> {
        let op_args = match other.op_args {
            Some(ref oargs) => {
                let mut v0 = Vec::with_capacity(oargs.len());
                for e in oargs {
                    let a = match OpArg::from(e, context) {
                        Err(err) => return Err(err),
                        Ok(a) => a,
                    };
                    v0.push(a);
                }
                Some(v0)
            }
            None => None,
        };
        Ok(FuncDecl {
            name: name,
            args: None,
            op_args: op_args,
            variant: FuncVariants::NonRelational,
        })
    }

    pub fn get_name(&self) -> &str {
        match self.name {
            Terminal::FreeTerm(var) => unsafe { &(&*var).name },
            Terminal::GroundedTerm(ref name) => name,
            Terminal::Keyword(name) => name,
        }
    }

    fn contains(&self, var: &Var) -> bool {
        if self.args.is_some() {
            for a in self.args.as_ref().unwrap() {
                match a {
                    &Predicate::FreeTerm(ref term) => {
                        if term.term == &*var as *const Var {
                            return true;
                        }
                    }
                    _ => continue,
                }
            }
        }
        if self.op_args.is_some() {
            for a in self.op_args.as_ref().unwrap() {
                if a.contains(var) {
                    return true;
                }
            }
        }
        false
    }

    pub fn var_in_pos(&self, var: *const Var) -> Option<usize> {
        if self.args.is_some() {
            for (i, a) in self.args.as_ref().unwrap().iter().enumerate() {
                match a {
                    &Predicate::FreeTerm(ref term) => {
                        if term.term == var {
                            return Some(i);
                        }
                    }
                    _ => continue,
                }
            }
        }
        None
    }

    pub fn term_in_pos(&self, var: &str) -> Option<usize> {
        if self.args.is_some() {
            for (i, a) in self.args.as_ref().unwrap().iter().enumerate() {
                match a {
                    &Predicate::GroundedTerm(ref term) => {
                        if term.term == var {
                            return Some(i);
                        }
                    }
                    _ => continue,
                }
            }
        }
        None
    }

    fn equal_to_grounded(&self,
                         agent: &agent::Representation,
                         assignments: &Option<&HashMap<*const Var, &agent::VarAssignment>>)
                         -> Option<bool> {
        match self.variant {
            FuncVariants::Relational => {}
            _ => panic!("simag: cannot compare non-relational functions"),
        }
        for a in self.args.as_ref().unwrap() {
            match a {
                &Predicate::FreeTerm(ref compare) => {
                    if assignments.is_none() {
                        return None;
                    }
                    let assignments = assignments.as_ref().unwrap();
                    if let Some(entity) = assignments.get(&compare.term) {
                        if let Ok(grfunc) = GroundedFunc::from_free(&self, assignments) {
                            if let Some(current) = entity.get_relationship(&grfunc) {
                                if current != &grfunc {
                                    return Some(false);
                                }
                            } else {
                                return None;
                            }
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                &Predicate::GroundedTerm(ref compare) => {
                    if let Some(current) = agent.get_entity_from_class(&compare.term) {
                        if current != compare {
                            return Some(false);
                        }
                    } else {
                        return None;
                    }
                }
            }
        }
        Some(true)
    }

    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<&HashMap<*const Var, &agent::VarAssignment>>) {
        let grfunc = GroundedFunc::from_free(&self, assignments.as_ref().unwrap());
        if grfunc.is_ok() {
            agent.up_relation(grfunc.unwrap());
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    name: Terminal,
    args: Vec<Predicate>,
    op_args: Option<Vec<OpArg>>,
}

impl<'a> ClassDecl {
    pub fn from(other: &ClassDeclBorrowed<'a>,
                context: &mut Context)
                -> Result<ClassDecl, ParseErrF> {
        let class_name = match Terminal::from(&other.name, context) {
            Ok(val) => val,
            Err(err) => return Err(err),
        };
        let args = {
            let mut v0 = Vec::with_capacity(other.args.len());
            for a in &other.args {
                let pred = Predicate::from(a, context, &class_name);
                if pred.is_err() {
                    return Err(pred.unwrap_err());
                }
                v0.push(pred.unwrap());
            }
            v0
        };
        let op_args = match other.op_args {
            Some(ref oargs) => {
                let mut v0 = Vec::with_capacity(oargs.len());
                for e in oargs {
                    let a = match OpArg::from(e, context) {
                        Err(err) => return Err(err),
                        Ok(a) => a,
                    };
                    v0.push(a);
                }
                Some(v0)
            }
            None => None,
        };
        Ok(ClassDecl {
            name: class_name,
            args: args,
            op_args: op_args,
        })
    }

    pub fn get_name(&self) -> &str {
        match self.name {
            Terminal::FreeTerm(var) => unsafe { &(&*var).name },
            Terminal::GroundedTerm(ref name) => name,
            Terminal::Keyword(name) => name,
        }
    }

    fn contains(&self, var: &Var) -> bool {
        for a in &self.args {
            match a {
                &Predicate::FreeTerm(ref term) => {
                    if term.term == &*var as *const Var {
                        return true;
                    }
                }
                _ => continue,
            }
        }
        if self.op_args.is_some() {
            for a in self.op_args.as_ref().unwrap() {
                if a.contains(var) {
                    return true;
                }
            }
        }
        false
    }

    fn equal_to_grounded(&self,
                         agent: &agent::Representation,
                         assignments: &Option<&HashMap<*const Var, &agent::VarAssignment>>)
                         -> Option<bool> {
        for a in &self.args {
            match a {
                &Predicate::FreeTerm(ref free) => {
                    if assignments.is_none() {
                        return None;
                    }
                    if let Some(entity) = assignments.as_ref().unwrap().get(&free.term) {
                        let grounded = entity.get_class(&free.parent.get_name());
                        if !free.equal_to_grounded(grounded) {
                            return Some(false);
                        }
                    } else {
                        return None;
                    }
                }
                &Predicate::GroundedTerm(ref compare) => {
                    if let Some(current) = agent.get_entity_from_class(&compare.term) {
                        if current != compare {
                            return Some(false);
                        }
                    } else {
                        return None;
                    }
                }
            }
        }
        Some(true)
    }

    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<&HashMap<*const Var, &agent::VarAssignment>>) {
        unimplemented!()
    }
}

#[derive(Debug, Clone)]
struct OpArg {
    term: OpArgTerm,
    comp: Option<(CompOperator, OpArgTerm)>,
}

impl<'a> OpArg {
    pub fn from(other: &OpArgBorrowed<'a>, context: &mut Context) -> Result<OpArg, ParseErrF> {
        let comp = match other.comp {
            Some((op, ref tors)) => {
                let t = OpArgTerm::from(&tors, context);
                if t.is_err() {
                    return Err(t.unwrap_err());
                }
                Some((op, t.unwrap()))
            }
            None => None,
        };
        let t = OpArgTerm::from(&other.term, context);
        if t.is_err() {
            return Err(t.unwrap_err());
        }
        Ok(OpArg {
            term: t.unwrap(),
            comp: comp,
        })
    }

    #[inline]
    fn contains(&self, var: &Var) -> bool {
        if self.term.is_var(var) {
            return true;
        }
        if let Some((_, ref term)) = self.comp {
            if term.is_var(var) {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone)]
enum OpArgTerm {
    Terminal(Terminal),
    String(String),
}

impl<'a> OpArgTerm {
    fn from(other: &OpArgTermBorrowed<'a>, context: &mut Context) -> Result<OpArgTerm, ParseErrF> {
        match *other {
            OpArgTermBorrowed::Terminal(slice) => {
                let t = match Terminal::from_slice(slice, context) {
                    Err(err) => return Err(err),
                    Ok(val) => val,
                };
                Ok(OpArgTerm::Terminal(t))
            }
            OpArgTermBorrowed::String(slice) => {
                Ok(OpArgTerm::String(String::from_utf8_lossy(slice).into_owned()))
            }
        }
    }

    #[inline]
    fn is_var(&self, var: &Var) -> bool {
        match *self {
            OpArgTerm::Terminal(ref term) => term.is_var(var),
            OpArgTerm::String(_) => false,
        }
    }
}

#[derive(Debug)]
pub struct Var {
    pub name: String,
    op_arg: Option<OpArg>,
}

impl Var {
    pub fn from<'a>(input: &VarBorrowed<'a>, context: &mut Context) -> Result<Var, ParseErrF> {
        let &VarBorrowed { name: TerminalBorrowed(name), ref op_arg } = input;
        let op_arg = match *op_arg {
            Some(ref op_arg) => {
                let t = match OpArg::from(op_arg, context) {
                    Err(err) => return Err(err),
                    Ok(v) => v,
                };
                Some(t)
            }
            None => None,
        };
        let name = unsafe { String::from(str::from_utf8_unchecked(name)) };
        if reserved(&name) {
            return Err(ParseErrF::ReservedKW(name));
        }
        Ok(Var {
            name: name,
            op_arg: op_arg,
        })
    }
}

#[derive(Debug)]
pub struct Skolem {
    pub name: String,
    op_arg: Option<OpArg>,
}

impl Skolem {
    pub fn from<'a>(input: &SkolemBorrowed<'a>,
                    context: &mut Context)
                    -> Result<Skolem, ParseErrF> {
        let &SkolemBorrowed { name: TerminalBorrowed(name), ref op_arg } = input;
        let op_arg = match *op_arg {
            Some(ref op_arg) => {
                let t = match OpArg::from(op_arg, context) {
                    Err(err) => return Err(err),
                    Ok(v) => v,
                };
                Some(t)
            }
            None => None,
        };
        let name = unsafe { String::from(str::from_utf8_unchecked(name)) };
        if reserved(&name) {
            return Err(ParseErrF::ReservedKW(name));
        }
        Ok(Skolem {
            name: name,
            op_arg: op_arg,
        })
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Terminal {
    FreeTerm(*const Var),
    GroundedTerm(String),
    Keyword(&'static str),
}

impl<'a> Terminal {
    fn from(other: &TerminalBorrowed<'a>, context: &mut Context) -> Result<Terminal, ParseErrF> {
        let &TerminalBorrowed(slice) = other;
        let name = unsafe { String::from(str::from_utf8_unchecked(slice)) };
        if reserved(&name) {
            return Err(ParseErrF::ReservedKW(name));
        }
        for v in &context.vars {
            let v_r: &Var = unsafe { &**v };
            if v_r.name == name {
                return Ok(Terminal::FreeTerm(*v));
            }
        }
        Ok(Terminal::GroundedTerm(name))
    }

    fn from_slice(slice: &[u8], context: &mut Context) -> Result<Terminal, ParseErrF> {
        let name = unsafe { String::from(str::from_utf8_unchecked(slice)) };
        if reserved(&name) {
            return Err(ParseErrF::ReservedKW(name));
        }
        for v in &context.vars {
            let v: &Var = unsafe { &**v };
            if v.name == name {
                return Ok(Terminal::FreeTerm(v));
            }
        }
        Ok(Terminal::GroundedTerm(name))
    }

    fn is_var(&self, v1: &Var) -> bool {
        match *self {
            Terminal::FreeTerm(v0) => {
                if (&*v1 as *const Var) == v0 {
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn get_name(&self) -> &str {
        match self {
            &Terminal::GroundedTerm(ref name) => name,
            _ => panic!("simag: attempted to get a name from a non-grounded terminal"),
        }
    }
}

fn reserved(s: &str) -> bool {
    match s {
        "let" => true,
        "time_calc" => true,
        "exists" => true,
        "fn" => true,
        "time" => true,
        _ => false,
    }
}
