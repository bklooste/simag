//! Stores a serie of logical atoms (be them predicates or
//! connectives), that form a well-formed logic formula. These are rulesets
//! for reasoning, cataloging objects into sets/classes, and the relationships
//! between these objects.
//!
//! LogSentence types are akin to minimal working compiled programs formed
//! by compounded expressions which will evaluate with the current knowledge
//! when called and perform any subtitution in the knowledge base if pertinent.
use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
use std::rc::Rc;
use std::fmt;
use std::iter::FromIterator;

use chrono::{UTC, DateTime};

use lang::parser::*;
use lang::common::*;
use agent;

/// Type to store a first-order logic complex sentence.
///
/// This sentence is the result of parsing a sentence and compile
/// it in an usable form for the agent to classify and reason about
/// objects and relations, cannot be instantiated directly.
///
/// It's callable when instantiated, accepts as arguments:
/// 1) the working knowledge-base
/// 2) n types which will subsitute the variables in the sentence
///    or a list of string.
#[derive(Debug)]
pub struct LogSentence {
    particles: Vec<Rc<Particle>>,
    produced: Vec<Rc<LogSentence>>,
    vars: Option<Vec<Rc<Var>>>,
    skolem: Option<Vec<Rc<Skolem>>>,
    root: Option<Rc<Particle>>,
    predicates: (Vec<Rc<Assert>>, Vec<Rc<Assert>>),
    pub var_req: Option<HashMap<Rc<Var>, Vec<Rc<Assert>>>>,
    pub created: DateTime<UTC>,
    id: Vec<u8>,
}

impl<'a> LogSentence {
    pub fn new(ast: &Next, context: &mut Context) -> Result<LogSentence, ParseErrF> {
        let mut sent = LogSentence {
            particles: Vec::new(),
            produced: Vec::new(),
            skolem: None,
            vars: None,
            root: None,
            predicates: (vec![], vec![]),
            var_req: None,
            created: UTC::now(),
            id: vec![],
        };
        let r = walk_ast(ast, &mut sent, context)?;
        sent.root = Some(r);
        // classify the kind of sentence and check that are correct
        if sent.vars.is_none() {
            if !context.iexpr() {
                context.stype = SentType::Rule;
            } else {
                return Err(ParseErrF::RuleInclICond(format!("{}", sent)));
            }
        } else if context.iexpr() {
            let mut lhs: Vec<Rc<Particle>> = vec![];
            if let Err(err) = correct_iexpr(&sent, &mut lhs) {
                return Err(err);
            }
            let lhs = HashSet::from_iter(lhs.iter().map(|x| x as *const Rc<Particle>));
            let mut rhs: HashSet<*const Rc<Particle>> = HashSet::new();
            for p in &sent.particles {
                if p.is_atom() {
                    rhs.insert(p as *const Rc<Particle>);
                }
            }
            let mut rhs_v: Vec<Rc<Assert>> = vec![];
            for p in rhs.difference(&lhs).map(|x| *x) {
                let p = unsafe { &**p };
                match p {
                    &Particle::Atom(ref p) => rhs_v.push(p.borrow().pred.clone()),
                    _ => {}
                }
            }
            let mut lhs_v: Vec<Rc<Assert>> = vec![];
            for p in lhs.into_iter() {
                let p = unsafe { &**p };
                match p {
                    &Particle::Atom(ref p) => lhs_v.push(p.borrow().pred.clone()),
                    _ => {}
                }
            }
            sent.predicates = (lhs_v, rhs_v);
            // add var requeriments
            let req = sent.get_var_requeriments();
            if req.len() > 0 {
                sent.var_req = Some(req);
            }
        }
        sent.generate_uid();
        Ok(sent)
    }

    pub fn solve(&self,
                 agent: &agent::Representation,
                 assignments: Option<HashMap<Rc<Var>, &agent::VarAssignment>>,
                 context: &mut agent::ProofResult) {
        let root = self.root.clone();
        if let Some(res) = root.as_ref().unwrap().solve(agent, &assignments) {
            if res {
                if root.as_ref().unwrap().is_icond() {
                    root.as_ref().unwrap().substitute(agent, &assignments, context, &true)
                }
                context.result = Some(true);
            } else {
                if root.as_ref().unwrap().is_icond() {
                    root.as_ref().unwrap().substitute(agent, &assignments, context, &false)
                }
                context.result = Some(false);
            }
        } else {
            context.result = None;
        }
    }

    pub fn extract_all_predicates(self) -> (Vec<Rc<Var>>, Vec<Rc<Assert>>) {
        let LogSentence { vars, particles, .. } = self;
        let mut preds = vec![];
        for p in particles {
            if p.is_atom() {
                preds.push(p.extract_assertion())
            }
        }
        (vars.unwrap(), preds)
    }

    pub fn get_all_predicates(&self) -> Vec<&Assert> {
        let mut v = self.get_lhs_predicates();
        let mut v_rhs = self.get_rhs_predicates();
        v.append(&mut v_rhs);
        v
    }

    pub fn get_rhs_predicates(&self) -> Vec<&Assert> {
        let mut v = vec![];
        for p in &self.predicates.1 {
            let p = &**p as &Assert;
            v.push(p);
        }
        v
    }

    pub fn get_lhs_predicates(&self) -> Vec<&Assert> {
        let mut v = vec![];
        for p in &self.predicates.0 {
            let p = &**p as &Assert;
            v.push(p);
        }
        v
    }

    /// Returns the requeriments a variable must meet to fit the criteria in a sentence.
    /// This just takes into consideration the LHS variables.
    fn get_var_requeriments(&self) -> HashMap<Rc<Var>, Vec<Rc<Assert>>> {
        let mut requeriments = HashMap::new();
        if self.vars.is_none() {
            return requeriments;
        }
        for var in self.vars.as_ref().unwrap() {
            let mut var_req = Vec::new();
            for p in &self.predicates.0 {
                if p.contains(var) {
                    var_req.push(p.clone())
                }
            }
            requeriments.insert(var.clone(), var_req);
        }
        requeriments
    }

    fn add_var(&mut self, var: Rc<Var>) {
        if self.vars.is_none() {
            self.vars = Some(Vec::new());
        }
        self.vars.as_mut().unwrap().push(var.clone())
    }

    fn add_skolem(&mut self, skolem: Rc<Skolem>) {
        if self.skolem.is_none() {
            self.vars = Some(Vec::new());
        }
        self.skolem.as_mut().unwrap().push(skolem.clone())
    }

    fn add_particle(&mut self, p: Rc<Particle>) {
        self.particles.push(p)
    }

    fn generate_uid(&mut self) {
        for a in self.particles.iter() {
            match &**a {
                &Particle::Conjunction(_) => self.id.push(0),
                &Particle::Disjunction(_) => self.id.push(1),
                &Particle::Equivalence(_) => self.id.push(2),
                &Particle::Implication(_) => self.id.push(3),
                &Particle::IndConditional(_) => self.id.push(4),
                &Particle::Atom(ref p) => {
                    let mut id_1 = p.borrow().get_id();
                    self.id.append(&mut id_1)
                }
            }
        }
    }

    pub fn get_id(&self) -> &[u8] {
        &self.id
    }
}

impl ::std::cmp::PartialEq for LogSentence {
    fn eq(&self, other: &LogSentence) -> bool {
        self.id == other.id
    }
}

impl ::std::cmp::Eq for LogSentence {}

impl ::std::hash::Hash for LogSentence {
    fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl fmt::Display for LogSentence {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let prelim: String = format!("Sentence({})", self.root.as_ref().unwrap());
        let mut breaks = Vec::new();
        let mut depth = 0_usize;
        use std::iter;
        let tab_times = |depth: usize| -> String { iter::repeat("    ").take(depth).collect() };
        let mut in_atom = false;
        for (i, c) in prelim.chars().enumerate() {
            if c == '(' {
                if (i >= 10) && (&prelim[i - 9..i] == "Predicate") {
                    in_atom = true;
                    continue;
                }
                depth += 1;
                let s = format!("\n{}", tab_times(depth));
                breaks.push((i + 1, s));
            } else if c == ')' {
                if in_atom {
                    in_atom = false;
                    continue;
                }
                depth -= 1;
                let s = format!("\n{}", tab_times(depth));
                breaks.push((i, s));
            } else if c == 'n' {
                if &prelim[i..i + 3] == "n1:" {
                    let s = format!("\n{}", tab_times(depth));
                    breaks.push((i, s))
                }
            }
        }
        let mut slices = Vec::new();
        let mut prev: usize = 0;
        for (pos, b) in breaks.drain(..) {
            slices.push(String::from(&prelim[prev..pos]));
            slices.push(b);
            prev = pos;
        }
        slices.push(String::from(&prelim[prev..]));
        let mut collected = String::new();
        for s in slices {
            collected.push_str(&s)
        }
        write!(f, "{}", collected)
    }
}

pub enum SentType {
    IExpr,
    Expr,
    Rule,
}

#[derive(Debug, Clone)]
struct LogicIndCond {
    parent: Option<Rc<Particle>>,
    next: Vec<Rc<Particle>>,
}

impl LogicIndCond {
    fn new() -> LogicIndCond {
        LogicIndCond {
            parent: None,
            next: Vec::with_capacity(2),
        }
    }

    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        let n0 = &*self.next[0];
        if let Some(res) = n0.solve(agent, assignments) {
            if res {
                Some(true)
            } else {
                Some(false)
            }
        } else {
            None
        }
    }

    #[inline]
    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>,
                  context: &mut agent::ProofResult,
                  rhs: &bool) {
        let n1 = &*self.next[1];
        n1.substitute(agent, assignments, context, rhs)
    }

    fn get_next(&self, pos: usize) -> Option<Rc<Particle>> {
        if pos == 0 && self.next.len() >= 1 {
            Some(self.next[0].clone())
        } else if pos == 1 && self.next.len() == 2 {
            Some(self.next[1].clone())
        } else {
            None
        }
    }
}

impl fmt::Display for LogicIndCond {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let n0 = match self.get_next(0) {
            Some(n0) => String::from(format!("{}", n0)),
            None => String::from("none"),
        };
        let n1 = match self.get_next(1) {
            Some(n1) => String::from(format!("{}", n1)),
            None => String::from("none"),
        };
        write!(f, "Conditional(n0: {}, n1: {})", n0, n1)
    }
}

#[derive(Debug, Clone)]
struct LogicEquivalence {
    parent: Option<Rc<Particle>>,
    next: Vec<Rc<Particle>>,
}

impl LogicEquivalence {
    fn new() -> LogicEquivalence {
        LogicEquivalence {
            parent: None,
            next: Vec::with_capacity(2),
        }
    }

    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        let n0 = &*self.next[0];
        let n1 = &*self.next[1];
        let n0_res;
        if let Some(res) = n0.solve(agent, assignments) {
            if res {
                n0_res = true;
            } else {
                n0_res = false;
            }
        } else {
            return None;
        }
        let n1_res;
        if let Some(res) = n1.solve(agent, assignments) {
            if res {
                n1_res = true;
            } else {
                n1_res = false;
            }
        } else {
            return None;
        }
        if n0_res == n1_res {
            Some(true)
        } else {
            Some(false)
        }
    }

    fn get_next(&self, pos: usize) -> Option<Rc<Particle>> {
        if pos == 0 && self.next.len() >= 1 {
            Some(self.next[0].clone())
        } else if pos == 1 && self.next.len() == 2 {
            Some(self.next[1].clone())
        } else {
            None
        }
    }
}

impl fmt::Display for LogicEquivalence {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let n0 = match self.get_next(0) {
            Some(n0) => String::from(format!("{}", n0)),
            None => String::from("none"),
        };
        let n1 = match self.get_next(1) {
            Some(n1) => String::from(format!("{}", n1)),
            None => String::from("none"),
        };
        write!(f, "Equivalence(n0: {}, n1: {})", n0, n1)
    }
}

#[derive(Debug, Clone)]
struct LogicImplication {
    parent: Option<Rc<Particle>>,
    next: Vec<Rc<Particle>>,
}

impl LogicImplication {
    fn new() -> LogicImplication {
        LogicImplication {
            parent: None,
            next: Vec::with_capacity(2),
        }
    }

    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        let n0 = &*self.next[0];
        let n1 = &*self.next[1];
        let n0_res;
        if let Some(res) = n0.solve(agent, assignments) {
            if res {
                n0_res = true;
            } else {
                n0_res = false;
            }
        } else {
            return None;
        }
        let n1_res;
        if let Some(res) = n1.solve(agent, assignments) {
            if res {
                n1_res = true;
            } else {
                n1_res = false;
            }
        } else {
            return None;
        }
        if n0_res && !n1_res {
            Some(false)
        } else {
            Some(true)
        }
    }

    fn get_next(&self, pos: usize) -> Option<Rc<Particle>> {
        if pos == 0 && self.next.len() >= 1 {
            Some(self.next[0].clone())
        } else if pos == 1 && self.next.len() == 2 {
            Some(self.next[1].clone())
        } else {
            None
        }
    }
}

impl fmt::Display for LogicImplication {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let n0 = match self.get_next(0) {
            Some(n0) => String::from(format!("{}", n0)),
            None => String::from("none"),
        };
        let n1 = match self.get_next(1) {
            Some(n1) => String::from(format!("{}", n1)),
            None => String::from("none"),
        };
        write!(f, "Implication(n0: {}, n1: {})", n0, n1)
    }
}

#[derive(Debug, Clone)]
struct LogicConjunction {
    parent: Option<Rc<Particle>>,
    next: Vec<Rc<Particle>>,
}

impl LogicConjunction {
    fn new() -> LogicConjunction {
        LogicConjunction {
            parent: None,
            next: Vec::with_capacity(2),
        }
    }

    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        let n0 = &*self.next[0];
        let n1 = &*self.next[1];
        if let Some(res) = n0.solve(agent, assignments) {
            if !res {
                return Some(false);
            }
        } else {
            return None;
        }
        if let Some(res) = n1.solve(agent, assignments) {
            if !res {
                return Some(false);
            }
        } else {
            return None;
        }
        Some(true)
    }

    #[inline]
    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>,
                  context: &mut agent::ProofResult,
                  rhs: &bool) {
        let n1 = &*self.next[1];
        n1.substitute(agent, assignments, context, rhs);
        let n0 = &*self.next[0];
        n0.substitute(agent, assignments, context, rhs);
    }

    fn get_next(&self, pos: usize) -> Option<Rc<Particle>> {
        if pos == 0 && self.next.len() >= 1 {
            Some(self.next[0].clone())
        } else if pos == 1 && self.next.len() == 2 {
            Some(self.next[1].clone())
        } else {
            None
        }
    }
}

impl fmt::Display for LogicConjunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let n0 = match self.get_next(0) {
            Some(n0) => String::from(format!("{}", n0)),
            None => String::from("none"),
        };
        let n1 = match self.get_next(1) {
            Some(n1) => String::from(format!("{}", n1)),
            None => String::from("none"),
        };
        write!(f, "Conjunction(n0: {}, n1: {})", n0, n1)
    }
}

#[derive(Debug, Clone)]
struct LogicDisjunction {
    parent: Option<Rc<Particle>>,
    next: Vec<Rc<Particle>>,
}

impl LogicDisjunction {
    fn new() -> LogicDisjunction {
        LogicDisjunction {
            parent: None,
            next: Vec::with_capacity(2),
        }
    }

    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        let n0 = &*self.next[0];
        let n1 = &*self.next[1];
        let n0_res;
        if let Some(res) = n0.solve(agent, assignments) {
            if res {
                n0_res = true;
            } else {
                n0_res = false;
            }
        } else {
            return None;
        }
        let n1_res;
        if let Some(res) = n1.solve(agent, assignments) {
            if res {
                n1_res = true;
            } else {
                n1_res = false;
            }
        } else {
            return None;
        }
        if n0_res != n1_res {
            Some(true)
        } else {
            Some(false)
        }
    }

    #[inline]
    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>,
                  context: &mut agent::ProofResult,
                  rhs: &bool) {
        if *rhs {
            let n1 = &*self.next[1];
            n1.substitute(agent, assignments, context, rhs)
        } else {
            let n0 = &*self.next[0];
            n0.substitute(agent, assignments, context, rhs)
        }
    }

    fn get_next(&self, pos: usize) -> Option<Rc<Particle>> {
        if pos == 0 && self.next.len() >= 1 {
            Some(self.next[0].clone())
        } else if pos == 1 && self.next.len() == 2 {
            Some(self.next[1].clone())
        } else {
            None
        }
    }
}

impl fmt::Display for LogicDisjunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let n0 = match self.get_next(0) {
            Some(n0) => String::from(format!("{}", n0)),
            None => String::from("none"),
        };
        let n1 = match self.get_next(1) {
            Some(n1) => String::from(format!("{}", n1)),
            None => String::from("none"),
        };
        write!(f, "Disjunction(n0: {}, n1: {})", n0, n1)
    }
}

#[derive(Debug, Clone)]
struct LogicAtom {
    parent: Option<Rc<Particle>>,
    pred: Rc<Assert>,
}

impl LogicAtom {
    fn new(term: Assert) -> LogicAtom {
        LogicAtom {
            parent: None,
            pred: Rc::new(term),
        }
    }

    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        if let Some(res) = self.pred.equal_to_grounded(agent, assignments) {
            if res {
                Some(true)
            } else {
                Some(false)
            }
        } else {
            None
        }
    }

    #[inline]
    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>,
                  context: &mut agent::ProofResult) {
        self.pred.substitute(agent, assignments, context)
    }

    fn get_name(&self) -> &str {
        self.pred.get_name()
    }

    #[inline]
    fn get_id(&self) -> Vec<u8> {
        self.pred.get_id()
    }
}

impl fmt::Display for LogicAtom {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Predicate({})", self.get_name())
    }
}

#[derive(Debug)]
enum Particle {
    Atom(RefCell<LogicAtom>),
    Conjunction(RefCell<LogicConjunction>),
    Disjunction(RefCell<LogicDisjunction>),
    Implication(RefCell<LogicImplication>),
    Equivalence(RefCell<LogicEquivalence>),
    IndConditional(RefCell<LogicIndCond>),
}

impl Particle {
    #[inline]
    fn solve(&self,
             agent: &agent::Representation,
             assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>)
             -> Option<bool> {
        match *self {
            Particle::Conjunction(ref p) => p.borrow().solve(agent, assignments),
            Particle::Disjunction(ref p) => p.borrow().solve(agent, assignments),
            Particle::Implication(ref p) => p.borrow().solve(agent, assignments),
            Particle::Equivalence(ref p) => p.borrow().solve(agent, assignments),
            Particle::IndConditional(ref p) => p.borrow().solve(agent, assignments),
            Particle::Atom(ref p) => p.borrow().solve(agent, assignments),
        }
    }

    #[inline]
    fn substitute(&self,
                  agent: &agent::Representation,
                  assignments: &Option<HashMap<Rc<Var>, &agent::VarAssignment>>,
                  context: &mut agent::ProofResult,
                  rhs: &bool) {
        match *self {
            Particle::IndConditional(ref p) => {
                p.borrow().substitute(agent, assignments, context, rhs)
            }
            Particle::Disjunction(ref p) => p.borrow().substitute(agent, assignments, context, rhs),
            Particle::Conjunction(ref p) => p.borrow().substitute(agent, assignments, context, rhs),
            Particle::Atom(ref p) => p.borrow().substitute(agent, assignments, context),
            _ => panic!("simag: wrong operator on the rhs of the expression"),
        }
    }

    #[inline]
    fn add_parent(&self, ptr: Rc<Particle>) {
        match *self {
            Particle::Conjunction(ref p) => {
                p.borrow_mut().parent = Some(ptr);
            }
            Particle::Disjunction(ref p) => {
                p.borrow_mut().parent = Some(ptr);
            }
            Particle::Implication(ref p) => {
                p.borrow_mut().parent = Some(ptr);
            }
            Particle::Equivalence(ref p) => {
                p.borrow_mut().parent = Some(ptr);
            }
            Particle::IndConditional(ref p) => {
                p.borrow_mut().parent = Some(ptr);
            }
            Particle::Atom(ref p) => {
                p.borrow_mut().parent = Some(ptr);
            }
        }
    }

    #[inline]
    fn get_next(&self, pos: usize) -> Option<Rc<Particle>> {
        match *self {
            Particle::Conjunction(ref p) => p.borrow().get_next(pos),
            Particle::Disjunction(ref p) => p.borrow().get_next(pos),
            Particle::Implication(ref p) => p.borrow().get_next(pos),
            Particle::Equivalence(ref p) => p.borrow().get_next(pos),
            Particle::IndConditional(ref p) => p.borrow().get_next(pos),
            Particle::Atom(_) => None,
        }
    }

    #[inline]
    fn extract_assertion(&self) -> Rc<Assert> {
        match *self {
            Particle::Atom(ref atom) => atom.borrow().pred.clone(),
            _ => panic!(),
        }
    }
    
    #[inline]
    fn is_atom(&self) -> bool {
        match *self {
            Particle::Atom(_) => true,
            _ => false,
        }
    }

    #[inline]
    fn is_icond(&self) -> bool {
        match *self {
            Particle::IndConditional(_) => true,
            _ => false,
        }
    }

    fn is_disjunction(&self) -> Result<(), ParseErrF> {
        match *self {
            Particle::Disjunction(_) => Ok(()),
            _ => Err(ParseErrF::IConnectAfterOr),
        }
    }

    fn is_conjunction(&self) -> Result<(), ParseErrF> {
        match *self {
            Particle::Conjunction(_) => Ok(()),
            _ => Err(ParseErrF::IConnectAfterOr),
        }
    }

    fn add_rhs(&self, next: Rc<Particle>) {
        match *self {
            Particle::Conjunction(ref p) => {
                if p.borrow().next.len() == 2 {
                    p.borrow_mut().next.pop();
                }
                p.borrow_mut().next.push(next)
            }
            Particle::Disjunction(ref p) => {
                if p.borrow().next.len() == 2 {
                    p.borrow_mut().next.pop();
                }
                p.borrow_mut().next.push(next)
            }
            Particle::Implication(ref p) => {
                if p.borrow().next.len() == 2 {
                    p.borrow_mut().next.pop();
                }
                p.borrow_mut().next.push(next)
            }
            Particle::Equivalence(ref p) => {
                if p.borrow().next.len() == 2 {
                    p.borrow_mut().next.pop();
                }
                p.borrow_mut().next.push(next)
            }
            Particle::IndConditional(ref p) => {
                if p.borrow().next.len() == 2 {
                    p.borrow_mut().next.pop();
                }
                p.borrow_mut().next.push(next)
            }
            _ => panic!("simag: expected an operator, found a predicate instead"),
        };
    }

    fn add_lhs(&self, next: Rc<Particle>) {
        match *self {
            Particle::Conjunction(ref p) => {
                if p.borrow_mut().next.len() == 1 {
                    p.borrow_mut().next.insert(0, next)
                } else if p.borrow_mut().next.len() == 0 {
                    p.borrow_mut().next.push(next)
                } else {
                    p.borrow_mut().next.remove(0);
                    p.borrow_mut().next.insert(0, next)
                }
            }
            Particle::Disjunction(ref p) => {
                if p.borrow_mut().next.len() == 1 {
                    p.borrow_mut().next.insert(0, next)
                } else if p.borrow_mut().next.len() == 0 {
                    p.borrow_mut().next.push(next)
                } else {
                    p.borrow_mut().next.remove(0);
                    p.borrow_mut().next.insert(0, next)
                }
            }
            Particle::Implication(ref p) => {
                if p.borrow_mut().next.len() == 1 {
                    p.borrow_mut().next.insert(0, next)
                } else if p.borrow_mut().next.len() == 0 {
                    p.borrow_mut().next.push(next)
                } else {
                    p.borrow_mut().next.remove(0);
                    p.borrow_mut().next.insert(0, next)
                }
            }
            Particle::Equivalence(ref p) => {
                if p.borrow_mut().next.len() == 1 {
                    p.borrow_mut().next.insert(0, next)
                } else if p.borrow_mut().next.len() == 0 {
                    p.borrow_mut().next.push(next)
                } else {
                    p.borrow_mut().next.remove(0);
                    p.borrow_mut().next.insert(0, next)
                }
            }
            Particle::IndConditional(ref p) => {
                if p.borrow_mut().next.len() == 1 {
                    p.borrow_mut().next.insert(0, next)
                } else if p.borrow_mut().next.len() == 0 {
                    p.borrow_mut().next.push(next)
                } else {
                    p.borrow_mut().next.remove(0);
                    p.borrow_mut().next.insert(0, next)
                }
            }
            _ => panic!("simag: expected an operator, found a predicate instead"),
        };
    }
}

impl fmt::Display for Particle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Particle::Atom(ref p) => write!(f, "{}", *p.borrow()),
            Particle::Conjunction(ref p) => write!(f, "{}", *p.borrow()),
            Particle::Disjunction(ref p) => write!(f, "{}", *p.borrow()),
            Particle::Equivalence(ref p) => write!(f, "{}", *p.borrow()),
            Particle::Implication(ref p) => write!(f, "{}", *p.borrow()),
            Particle::IndConditional(ref p) => write!(f, "{}", *p.borrow()),
        }
    }
}

// infrastructure to construct compiled logsentences:

pub struct Context {
    pub stype: SentType,
    pub vars: Vec<Rc<Var>>,
    pub skols: Vec<Rc<Skolem>>,
    aliasing_vars: HashMap<Rc<Var>, (usize, Rc<Var>)>,
    aliasing_skols: HashMap<Rc<Skolem>, (usize, Rc<Skolem>)>,
    from_chain: bool,
    in_rhs: bool,
    pub in_assertion: bool,
    pub is_tell: bool,
}

impl Context {
    pub fn new() -> Context {
        Context {
            vars: Vec::new(),
            skols: Vec::new(),
            stype: SentType::Expr,
            in_rhs: true,
            from_chain: false,
            aliasing_vars: HashMap::new(),
            aliasing_skols: HashMap::new(),
            in_assertion: false,
            is_tell: false,
        }
    }

    fn iexpr(&self) -> bool {
        match self.stype {
            SentType::Expr => false,
            SentType::IExpr => true,
            SentType::Rule => false,
        }
    }
}

fn walk_ast(ast: &Next,
            sent: &mut LogSentence,
            context: &mut Context)
            -> Result<Rc<Particle>, ParseErrF> {
    match *ast {
        Next::Assert(ref decl) => {
            let particle = Rc::new(match decl {
                &AssertBorrowed::ClassDecl(ref decl) => {
                    let cls = match ClassDecl::from(decl, context) {
                        Err(err) => return Err(err),
                        Ok(cls) => cls,
                    };
                    let atom = LogicAtom::new(Assert::ClassDecl(cls));
                    Particle::Atom(RefCell::new(atom))
                }
                &AssertBorrowed::FuncDecl(ref decl) => {
                    let func = match FuncDecl::from(decl, context) {
                        Err(err) => return Err(err),
                        Ok(func) => func,
                    };
                    let atom = LogicAtom::new(Assert::FuncDecl(func));
                    Particle::Atom(RefCell::new(atom))
                }
            });
            sent.add_particle(particle.clone());
            context.from_chain = false;
            Ok(particle)
        }
        Next::ASTNode(ref ast) => {
            let mut v_cnt = 0;
            let mut s_cnt = 0;
            let mut swap_vars: Vec<(usize, Rc<Var>, Rc<Var>)> = Vec::new();
            let mut swap_skolem: Vec<(usize, Rc<Skolem>, Rc<Skolem>)> = Vec::new();

            fn drop_local_vars(context: &mut Context, v_cnt: usize) {
                let l = context.vars.len() - v_cnt;
                let local_vars = context.vars.drain(l..).collect::<Vec<Rc<Var>>>();
                for v in local_vars {
                    if context.aliasing_vars.contains_key(&v) {
                        let (idx, aliased) = context.aliasing_vars.remove(&v).unwrap();
                        context.vars.insert(idx, aliased);
                    }
                }
            }

            fn drop_local_skolems(context: &mut Context, s_cnt: usize) {
                let l = context.skols.len() - s_cnt;
                let local_skolem = context.skols.drain(l..).collect::<Vec<Rc<Skolem>>>();
                for v in local_skolem {
                    if context.aliasing_skols.contains_key(&v) {
                        let (idx, aliased) = context.aliasing_skols.remove(&v).unwrap();
                        context.skols.insert(idx, aliased);
                    }
                }
            }

            // make vars and add to sent, also add them to local scope context
            if ast.vars.is_some() {
                for v in ast.vars.as_ref().unwrap() {
                    match *v {
                        // if there is a var in context with the current name, alias it
                        VarDeclBorrowed::Var(ref v) => {
                            let var = match Var::from(v, context) {
                                Err(err) => return Err(err),
                                Ok(val) => Rc::new(val),
                            };
                            for (i, v) in context.vars.iter().enumerate() {
                                if v.name == var.name {
                                    swap_vars.push((i, v.clone(), var.clone()));
                                }
                            }
                            context.vars.push(var.clone());
                            v_cnt += 1;
                            sent.add_var(var);
                        }
                        VarDeclBorrowed::Skolem(ref s) => {
                            let skolem = match Skolem::from(s, context) {
                                Err(err) => return Err(err),
                                Ok(val) => Rc::new(val),
                            };
                            for (i, v) in context.skols.iter().enumerate() {
                                if v.name == skolem.name {
                                    swap_skolem.push((i, v.clone(), skolem.clone()));
                                }
                            }
                            context.skols.push(skolem.clone());
                            s_cnt += 1;
                            sent.add_skolem(skolem.clone());
                        }
                    }
                }
                for &(i, ref aliased, ref var) in &swap_vars {
                    context.vars.remove(i);
                    context.aliasing_vars.insert(var.clone(), (i, aliased.clone()));
                }
                for &(i, ref aliased, ref var) in &swap_skolem {
                    context.skols.remove(i);
                    context.aliasing_skols.insert(var.clone(), (i, aliased.clone()));
                }
            }
            if ast.logic_op.is_some() {
                let op = Rc::new(match ast.logic_op.as_ref().unwrap() {
                    &LogicOperator::ICond => {
                        context.stype = SentType::IExpr;
                        Particle::IndConditional(RefCell::new(LogicIndCond::new()))
                    }
                    &LogicOperator::And => {
                        Particle::Conjunction(RefCell::new(LogicConjunction::new()))
                    }
                    &LogicOperator::Or => {
                        Particle::Disjunction(RefCell::new(LogicDisjunction::new()))
                    }
                    &LogicOperator::Implication => {
                        Particle::Implication(RefCell::new(LogicImplication::new()))
                    }
                    &LogicOperator::Biconditional => {
                        Particle::Equivalence(RefCell::new(LogicEquivalence::new()))
                    }
                });
                let next = match walk_ast(&ast.next, sent, context) {
                    Ok(opt) => opt,
                    Err(err) => return Err(err),
                };
                drop_local_vars(context, v_cnt);
                drop_local_skolems(context, s_cnt);
                next.add_parent(op.clone());
                if context.in_rhs {
                    op.add_rhs(next);
                } else {
                    op.add_lhs(next);
                }
                sent.add_particle(op.clone());
                Ok(op)
            } else {
                let res = walk_ast(&ast.next, sent, context);
                drop_local_vars(context, v_cnt);
                drop_local_skolems(context, s_cnt);
                res
            }
        }
        Next::Chain(ref nodes) => {
            if nodes.len() == 2 {
                let in_side = context.in_rhs;
                // walk lhs
                context.in_rhs = false;
                let lhs_ptr = match walk_ast(&nodes[0], sent, context) {
                    Ok(ptr) => ptr,
                    Err(err) => return Err(err),

                };
                let lhs_r = lhs_ptr.clone();
                let lhs_is_atom = lhs_r.is_atom();
                // walk rhs
                context.in_rhs = true;
                let rhs_ptr = match walk_ast(&nodes[1], sent, context) {
                    Ok(ptr) => ptr,
                    Err(err) => return Err(err),
                };
                let rhs_r = rhs_ptr.clone();
                let rhs_is_atom = rhs_r.is_atom();
                // lhs is connective and rhs isn't
                let return_rhs;
                if !lhs_is_atom && rhs_is_atom {
                    return_rhs = false;
                    rhs_ptr.add_parent(lhs_ptr.clone());
                    lhs_r.add_rhs(rhs_ptr.clone());
                } else if lhs_is_atom && !rhs_is_atom {
                    return_rhs = true;
                    lhs_ptr.add_parent(rhs_ptr.clone());
                    rhs_r.add_lhs(lhs_ptr.clone());
                } else {
                    if context.from_chain {
                        // rhs comes from a chain, parent is lhs op
                        return_rhs = false;
                        rhs_ptr.add_parent(lhs_ptr.clone());
                        lhs_r.add_rhs(rhs_ptr.clone());
                    } else {
                        // lhs comes from a chain, parent is rhs op
                        return_rhs = true;
                        lhs_ptr.add_parent(rhs_ptr.clone());
                        rhs_r.add_lhs(lhs_ptr.clone());
                    }
                }
                context.in_rhs = in_side;
                context.from_chain = true;
                if return_rhs {
                    Ok(rhs_ptr)
                } else {
                    Ok(lhs_ptr)
                }
            } else {
                let len = nodes.len() - 1;
                let first = walk_ast(&nodes[0], sent, context)?;
                let operator;
                match &*first.clone() {
                    &Particle::Conjunction(_) => {
                        operator = LogicOperator::And;
                    }
                    &Particle::Disjunction(_) => {
                        operator = LogicOperator::Or;
                    }
                    _ => return Err(ParseErrF::IConnectInChain),
                }
                let mut prev = first.clone();
                if operator.is_and() {
                    for i in 1..len {
                        let ptr = walk_ast(&nodes[i], sent, context)?;
                        let a = ptr.clone();
                        let is_conj = a.is_conjunction();
                        if is_conj.is_err() {
                            return Err(is_conj.unwrap_err());
                        } else {
                            ptr.add_parent(prev.clone());
                            prev.add_rhs(ptr.clone());
                            prev = ptr;
                        }
                    }
                } else {
                    for i in 1..len {
                        let ptr = walk_ast(&nodes[i], sent, context)?;
                        let a = ptr.clone();
                        let is_disj = a.is_disjunction();
                        if is_disj.is_err() {
                            return Err(is_disj.unwrap_err());
                        } else {
                            ptr.add_parent(prev.clone());
                            prev.add_rhs(ptr.clone());
                            prev = ptr;
                        }
                    }
                }
                let last = walk_ast(&nodes[len], sent, context)?;
                last.add_parent(prev.clone());
                prev.add_rhs(last);
                context.from_chain = true;
                Ok(first)
            }
        }
        Next::None => Err(ParseErrF::WrongDef),
    }
}

fn correct_iexpr(sent: &LogSentence,
                 lhs: &mut Vec<Rc<Particle>>)
                 -> Result<(), ParseErrF> {

    fn has_icond_child(p: &Particle,
                       lhs: &mut Vec<Rc<Particle>>)
                       -> Result<(), ParseErrF> {
        if let Some(n1_0) = p.get_next(0) {
            match &*n1_0 {
                &Particle::IndConditional(_) => return Err(ParseErrF::IExprICondLHS),
                &Particle::Atom(_) => {
                    lhs.push(n1_0.clone());
                }
                _ => {}
            }
            has_icond_child(&*n1_0, lhs)?;
        }
        if let Some(n1_1) = p.get_next(1) {
            match &*n1_1 {
                &Particle::IndConditional(_) => return Err(ParseErrF::IExprICondLHS),
                &Particle::Atom(_) => {
                    lhs.push(n1_1.clone());
                }
                _ => {}
            }
            has_icond_child(&*n1_1, lhs)?;
        }
        Ok(())
    }

    fn wrong_operator(p: &Particle,
                      lhs: &mut Vec<Rc<Particle>>)
                      -> Result<(), ParseErrF> {
        if let Some(n1_0) = p.get_next(0) {
            // test that the lhs does not include any indicative conditional
            if n1_0.is_icond() {
                return Err(ParseErrF::IExprICondLHS);
            } else if n1_0.is_atom() {
                lhs.push(n1_0.clone());
            }
            has_icond_child(&*n1_0, lhs)?;
        }
        // test that the rh-most-s does include only icond or 'OR' connectives
        let mut is_wrong = Ok(());
        if let Some(n1_1) = p.get_next(1) {
            match &*n1_1 {
                &Particle::IndConditional(_) => {}
                &Particle::Disjunction(_) => {}
                &Particle::Conjunction(_) => {}
                &Particle::Atom(_) => {}
                _ => return Err(ParseErrF::IExprWrongOp),
            }
            is_wrong = wrong_operator(&*n1_1, lhs);
        }
        is_wrong
    }

    let first: &Particle = &*sent.root.as_ref().unwrap();
    match first {
        &Particle::IndConditional(_) => {}
        _ => return Err(ParseErrF::IExprNotIcond),
    }
    if let Some(n1_0) = first.get_next(0) {
        match &*n1_0 {
            &Particle::IndConditional(_) => return Err(ParseErrF::IExprICondLHS),
            _ => {}
        }
    }
    for p in &sent.particles {
        match **p {
            Particle::Atom(ref atom) => {
                if !atom.borrow().pred.parent_is_grounded() {
                    return Err(ParseErrF::WrongPredicate);
                }
            }
            _ => {}
        }
    }
    wrong_operator(first, lhs)
}

#[cfg(test)]
mod test {
    use super::Particle;
    use lang::parser::*;

    #[test]
    fn icond_exprs() {
        let source = String::from("
            # Err:
            ((let x y z)
             ( ( cde[x,u=1] |> fn::fgh[y,u>0.5;x;z] ) |> hij[y,u=1] )
            )

            # Err:
            ((let x y z)
             ( abc[x,u=1]  |> (( cde[x,u=1] |> fn::fgh[y,u>0.5;x;z] ) && hij[y,u=1] ))
            )

            # Ok:
            ((let x y z)
             ( abc[x,u=1]  |> (
                 ( cde[x,u=1] && fn::fgh[y,u>0.5;x;z] ) |> hij[y,u=1]
             )))

            # Ok:
            (( let x y z )
             (( american[x,u=1] && weapon[y,u=1] && fn::sells[y,u>0.5;x;z] ) |> criminal[x,u=1]))
        ");

        let tree = Parser::parse(source, true);
        assert!(tree.is_ok());
        let mut tree = tree.unwrap();

        match tree.pop_front().unwrap() {
            ParseTree::ParseErr(ParseErrF::IExprICondLHS) => {}
            ParseTree::IExpr(sent) => {
                println!("@failed_err: {}", sent);
                panic!()
            }
            _ => panic!(),
        }

        match tree.pop_front().unwrap() {
            ParseTree::ParseErr(ParseErrF::IExprICondLHS) => {}
            ParseTree::IExpr(sent) => {
                println!("@failed_err: {}", sent);
                panic!()
            }
            _ => panic!(),
        }

        match tree.pop_front().unwrap() {
            ParseTree::IExpr(_) => {}
            _ => panic!(),
        }

        let sent = match tree.pop_front().unwrap() {
            ParseTree::IExpr(sent) => sent,
            _ => panic!(),
        };
        let root = &**(sent.root.as_ref().unwrap());
        match root {
            &Particle::IndConditional(ref p) => {
                match &*(p.borrow().next[0]) {
                    &Particle::Conjunction(ref op) => {
                        match &*(op.borrow().next[0]) {
                            &Particle::Atom(ref atm) => {
                                assert_eq!(atm.borrow().get_name(), "american");
                            }
                            _ => panic!(),
                        };
                        match &*(op.borrow().next[1]) {
                            &Particle::Conjunction(ref op) => {
                                match &*(op.borrow().next[0]) {
                                    &Particle::Atom(ref atm) => {
                                        assert_eq!(atm.borrow().get_name(), "weapon")
                                    }
                                    _ => panic!(),
                                };
                                match &*(op.borrow().next[1]) {
                                    &Particle::Atom(ref atm) => {
                                        assert_eq!(atm.borrow().get_name(), "sells");
                                    }
                                    _ => panic!(),
                                };
                            }
                            _ => panic!(),
                        }
                    }
                    _ => panic!(),
                }
                match &*(p.borrow().next[1]) {
                    &Particle::Atom(ref atm) => assert_eq!(atm.borrow().get_name(), "criminal"),
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }
}
