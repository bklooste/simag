use std::str;
use std::collections::HashMap;

use lang::parser::*;
use lang::logsent::*;
use agent::Representation;

// Predicate types:

#[derive(Debug, PartialEq, Clone)]
enum Predicate {
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
                let t = GroundedTerm::new(gt, a.uval, func_name, None);
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
           _: Option<Vec<i32>>)
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
            op = Some(uval.op);
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
            panic!("simag: one of the two grounded terms does not include a value")
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
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
           _: Option<Vec<i32>>)
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

    fn equal_to_grounded(&self, other: &GroundedTerm) -> bool {}
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
                             agent: &Representation,
                             assignments: &Option<&HashMap<*const Var, &GroundedTerm>>)
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
}

#[derive(Debug, PartialEq, Clone)]
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

    fn get_name(&self) -> &str {
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

    fn equal_to_grounded(&self,
                         agent: &Representation,
                         assignments: &Option<&HashMap<*const Var, &GroundedTerm>>)
                         -> Option<bool> {
        // #class_decl:
        // name: Terminal,
        // args: Option<Vec<Predicate>>,
        // op_args: Option<Vec<OpArg>>,
        // variant: FuncVariants,
        //
        // #free_term:
        // term: *const Var,
        // value: Option<f32>,
        // operator: Option<CompOperator>,
        // parent: Terminal,
        // dates: Option<Vec<i32>>,
        //
        // #grounded_term:
        // term: String,
        // value: Option<f32>,
        // operator: Option<CompOperator>,
        // parent: Terminal,
        // dates: Option<Vec<i32>>,
        //
        None
    }
}

#[derive(Debug, PartialEq, Clone)]
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

    fn get_name(&self) -> &str {
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
                         agent: &Representation,
                         assignments: &Option<&HashMap<*const Var, &GroundedTerm>>)
                         -> Option<bool> {
        for a in &self.args {
            match a {
                &Predicate::FreeTerm(ref compare) => {
                    if assignments.is_none() {
                        return None;
                    }
                    if assignments.as_ref().unwrap().contains_key(&compare.term) {
                        let grounded = assignments.as_ref().unwrap().get(&compare.term);
                        if !compare.equal_to_grounded(grounded.unwrap()) {
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
                    }
                }
            }
        }
        Some(true)
    }
}

#[derive(Debug, PartialEq, Clone)]
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

#[derive(Debug, PartialEq, Clone)]
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

#[derive(Debug, PartialEq)]
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
enum Terminal {
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
