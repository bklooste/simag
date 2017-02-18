//! # Grammar for the SIMAG declarative language
//! ```BNF
//! @comments			::	regex: \/\*(.*)\*\/ (multiline)
//! @eol_comments 		::	/#.*?$/
//!
//! scope = '(' ${var_decl}* ${skol_decl}*
//!             (class_decl | func_decl | scope | multiple)
//!             logic_op (class_decl | func_decl | scope | multiple)
//!         ')' ;
//! multiple = '(' ${(func_decl | class_decl)} (or_op | and_op) + ')' ;
//! var_decl = '(' 'let ' ${term [':' op_arg]','+ ')' ;
//! skol_decl = '(' 'exists ' ${term [':' op_arg]}','+ ')' ;
//! class_decl = term ['(' ${op_arg}','+ ')'] args ;
//! func_decl = 'fn::' term ['(' ${op_arg}','+ ')'] args
//! 		  | 'fn::' term '(' ${op_arg}','+ ')' ;
//! args = '[' ${ arg }';'+ ']';
//! arg = term [',' uval] ;
//! uval = 'u' comp_op number ;
//! op_arg = (string|term) [comp_op (string|term)] ;
//! icond_op    =	'|>' ;
//! and_op      =	'&&' ;
//! or_op		=	'||' ;
//! logic_op	=    '<=>'
//! 		    |    '=>'
//!             |    or_op
//!             |	 and_op ;
//! comp_op	= ('=' | '<' | '>') ;
//! term = regex: \$?[a-zA-Z0-9_]+ ;
//! number = regex: -?[0-9\.]+ ;
//! string = regex: ".*?"|'.*?' ;
//! ```

use std::str;
use std::str::FromStr;
use std::collections::VecDeque;
use std::thread;

use nom::{ErrorKind, IResult};
use nom::{is_alphanumeric, is_digit};
use nom;

use lang::logsent::*;
use lang::common::*;
use lang::errors::ParseErrF;

const ICOND_OP: &'static [u8] = b"|>";
const AND_OP: &'static [u8] = b"&&";
const OR_OP: &'static [u8] = b"||";
const IFF_OP: &'static [u8] = b"<=>";
const IMPL_OP: &'static [u8] = b"=>";

pub struct Parser;
impl Parser {
    /// Lexerless (mostly) recursive descent parser. Takes a string and outputs a correct ParseTree.
    pub fn parse(mut input: String, tell: bool) -> Result<VecDeque<ParseTree>, ParseErrF> {
        // store is a vec where the sequence of characters after cleaning up comments
        // will be stored, both have to be extended to 'static lifetime so they can be
        fn extend_lifetime<'b, T>(r: &'b mut T) -> &'static mut T {
            unsafe { ::std::mem::transmute::<&'b mut T, &'static mut T>(r) }
        }
        let input = extend_lifetime(&mut input);
        let mut store = vec![];
        let store_ref = extend_lifetime(&mut store);
        let scopes = match Self::feed(input.as_bytes(), store_ref) {
            Ok(scopes) => scopes,
            Err(err) => return Err(ParseErrF::from(err)),
        };
        // walk the AST output and, if correct, output a final parse tree
        let mut parse_trees = vec![];
        for ast in scopes {
            let res = thread::spawn(move || ParseTree::process_ast(ast, tell));
            parse_trees.push(res);
        }
        let results: VecDeque<ParseTree> = parse_trees.drain(..)
            .map(|res| {
                let res = res.join();
                match res.unwrap() {
                    Ok(ptr) => unsafe { *Box::from_raw(ptr as *mut ParseTree) },
                    Err(err) => ParseTree::ParseErr(err),
                }
            })
            .collect();
        Ok(results)
    }

    /// First pass: will tokenize and output an AST
    fn feed<'a>(input: &'a [u8], p2: &'a mut Vec<u8>) -> Result<Vec<Next<'a>>, ParseErrB<'a>> {
        // clean up every comment to facilitate further parsing
        let p1 = match remove_comments(input) {
            IResult::Done(_, done) => done,
            IResult::Error(nom::Err::Position(_, p)) => return Err(ParseErrB::UnclosedComment(p)),
            _ => return Err(ParseErrB::SyntaxErrorU),
        };
        for v in p1 {
            for b in v {
                p2.push(*b)
            }
        }

        // separate by scopes; everything inside a scope is either an other scope,
        // a var declaration, an operator or a terminal AST node;
        // inner scopes are linked by logical operators, and the terminal nodes contain
        // function and/or class declarations (which can be chained by AND operators);
        // a program consist of one ore more scopes (each representing a compounded expression)
        let scopes = get_blocks(&p2[..]);
        if scopes.is_err() {
            match scopes.unwrap_err() {
                nom::Err::Position(t, p) => {
                    match (t, p) {
                        (ErrorKind::Custom(0), p) => Err(ParseErrB::NonTerminal(p)),
                        (ErrorKind::Custom(1), p) => Err(ParseErrB::NonNumber(p)),
                        (ErrorKind::Custom(11), p) => Err(ParseErrB::NotScope(p)),
                        (ErrorKind::Custom(12), p) => Err(ParseErrB::UnbalDelim(p)),
                        (ErrorKind::Custom(15), p) => Err(ParseErrB::IllegalChain(p)),
                        (_, p) => Err(ParseErrB::SyntaxErrorPos(p)),
                    }
                }
                _ => Err(ParseErrB::SyntaxErrorU),
            }
        } else {
            let (_, scopes) = scopes.unwrap();
            Ok(scopes)
        }
    }
}

#[derive(Debug)]
pub enum ParseErrB<'a> {
    SyntaxError(Box<ParseErrB<'a>>),
    SyntaxErrorU,
    SyntaxErrorPos(&'a [u8]),
    NotScope(&'a [u8]),
    UnbalDelim(&'a [u8]),
    ExpectOpenDelim(&'a [u8]),
    IllegalChain(&'a [u8]),
    NonTerminal(&'a [u8]),
    NonNumber(&'a [u8]),
    UnclosedComment(&'a [u8]),
}

#[derive(Debug)]
pub enum ParseTree {
    Assertion(Vec<Assert>),
    IExpr(LogSentence),
    Expr(LogSentence),
    ParseErr(ParseErrF),
}

impl ParseTree {
    fn process_ast(input: Next, tell: bool) -> Result<usize, ParseErrF> {
        // return type is a hack to avoid compiling error due to not being 'Send' compatible
        // instead we return a raw pointer as usize and when unifying all the thread
        // results convert back to box and deref to the stack
        let mut context = Context::new();
        context.in_assertion = true;
        context.is_tell = tell;
        if let Ok(Some(tree)) = input.is_assertion(&mut context) {
            let ptr = Box::into_raw(Box::new(tree)) as usize;
            return Ok(ptr);
        }
        // it's an expression, make logic sentence from nested expressions
        let mut context = Context::new();
        context.is_tell = tell;
        match LogSentence::new(&input, &mut context) {
            Ok(sent) => {
                match context.stype {
                    SentType::IExpr => {
                        let ptr = Box::into_raw(Box::new(ParseTree::IExpr(sent))) as usize;
                        Ok(ptr)
                    }
                    SentType::Expr if context.is_tell => {
                        Err(ParseErrF::ExprWithVars(format!("{}", sent)))
                    }
                    SentType::Rule | SentType::Expr => {
                        let ptr = Box::into_raw(Box::new(ParseTree::Expr(sent))) as usize;
                        Ok(ptr)
                    }
                }
            }
            Err(err) => Err(ParseErrF::LogSentErr(err)),
        }
    }

    #[allow(dead_code)]
    pub fn is_err(&self) -> bool {
        match *self {
            ParseTree::ParseErr(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct ASTNode<'a> {
    pub next: Next<'a>,
    pub vars: Option<Vec<VarDeclBorrowed<'a>>>,
    pub logic_op: Option<LogicOperator>,
}

impl<'a> ASTNode<'a> {
    fn is_assertion(&self, context: &mut Context) -> Result<Option<ParseTree>, ParseErrF> {
        if self.vars.is_some() {
            return Ok(None);
        }
        self.next.is_assertion(context)
    }
}

#[derive(Debug)]
pub enum Next<'a> {
    Assert(AssertBorrowed<'a>),
    ASTNode(Box<ASTNode<'a>>),
    Chain(Vec<Next<'a>>),
    None,
}

impl<'a> Next<'a> {
    fn is_assertion(&self, context: &mut Context) -> Result<Option<ParseTree>, ParseErrF> {
        match *self {
            Next::Assert(ref decl) => {
                match *decl {
                    AssertBorrowed::ClassDecl(ref decl) => {
                        let cls = ClassDecl::from(decl, context)?;
                        Ok(Some(ParseTree::Assertion(vec![Assert::ClassDecl(cls)])))
                    }
                    AssertBorrowed::FuncDecl(ref decl) => {
                        let func = FuncDecl::from(decl, context)?;
                        Ok(Some(ParseTree::Assertion(vec![Assert::FuncDecl(func)])))
                    }
                }
            }
            Next::Chain(ref multi_decl) => {
                let mut v0: Vec<Assert> = Vec::with_capacity(multi_decl.len());
                // chek that indeed all elements are indeed assertions
                // avoid creating declarations prematurely
                for decl in multi_decl {
                    let d = decl.is_assertion(context);
                    match d {
                        Err(err) => return Err(err),
                        Ok(Some(ParseTree::Assertion(mut inner))) => {
                            for e in inner.drain(..) {
                                v0.push(e)
                            }
                        }
                        _ => return Ok(None),
                    }
                }
                Ok(Some(ParseTree::Assertion(v0)))
            }
            Next::ASTNode(ref node) => {
                let a: Result<Option<ParseTree>, ParseErrF> = (**node).is_assertion(context);
                match a {
                    Err(err) => Err(err),
                    Ok(Some(ParseTree::Assertion(assert))) => {
                        Ok(Some(ParseTree::Assertion(assert)))
                    }
                    _ => Ok(None),
                }
            }
            Next::None => Ok(None),
        }
    }
}

#[derive(Debug)]
pub enum AssertBorrowed<'a> {
    FuncDecl(FuncDeclBorrowed<'a>),
    ClassDecl(ClassDeclBorrowed<'a>),
}

#[derive(Debug)]
pub enum VarDeclBorrowed<'a> {
    Var(VarBorrowed<'a>),
    Skolem(SkolemBorrowed<'a>),
}

fn get_blocks(input: &[u8]) -> IResult<&[u8], Vec<Next>> {
    let input = remove_multispace(input);
    if input.len() == 0 {
        // empty program
        return IResult::Done(&b" "[..], vec![]);
    }
    // find the positions of the closing delimiters and try until it fails
    let mut results = Vec::new();
    let mut mcd = ::std::collections::VecDeque::new();
    let mut lp = 0;
    let mut rp = 0;
    let mut slp = -1_i64;
    for (i, c) in input.iter().enumerate() {
        if *c == b'(' {
            lp += 1;
            if slp < 0 {
                slp = i as i64;
            }
        } else if *c == b')' {
            rp += 1;
            if rp == lp {
                if i + 1 < input.len() {
                    mcd.push_back((slp as usize, i + 1));
                } else {
                    mcd.push_back((slp as usize, input.len()));
                }
                slp = -1;
            }
        }
    }
    if lp != rp {
        return IResult::Error(nom::Err::Position(ErrorKind::Custom(12), input));
    } else if mcd.is_empty() {
        return IResult::Error(nom::Err::Position(ErrorKind::Custom(11), input));
    }
    for _ in 0..mcd.len() {
        let (lp, rp) = mcd.pop_front().unwrap();
        match scope(&input[lp..rp]) {
            IResult::Done(_, done) => results.push(done),
            IResult::Error(err) => return IResult::Error(err),
            IResult::Incomplete(_) => return IResult::Error(nom::Err::Code(ErrorKind::Count)),
        }
    }
    if results.is_empty() {
        return IResult::Error(nom::Err::Position(ErrorKind::Custom(11), input));
    }
    IResult::Done(&b" "[..], results)
}

// scope disambiguation infrastructure:
#[allow(cyclomatic_complexity)]
fn scope(input: &[u8]) -> IResult<&[u8], Next> {
    fn take_vars(input: &[u8]) -> (usize, Option<Vec<VarDeclBorrowed>>) {
        named!(tv(&[u8]) -> Vec<Vec<VarDeclBorrowed>>, many1!(chain!(
            take_while!(is_multispace)? ~
            v1: alt!(variable | skolem) ,
            || { v1 }
        )));
        let output = tv(input);
        let vars;
        let mut offset = 0_usize;
        if output.is_done() {
            let (r, mut v) = output.unwrap();
            offset = input.len() - r.len();
            let mut v0 = Vec::new();
            // flat vec
            for ref mut v1 in v.drain(..) {
                for v2 in v1.drain(..) {
                    v0.push(v2);
                }
            }
            vars = Some(v0);
        } else {
            vars = None;
        }
        (offset, vars)
    }

    // check that it is indeed an scope
    let od = tuple!(input, opt!(take_while!(is_multispace)), char!('('));
    let input = match od {
        IResult::Done(rest, _) => rest,
        _ => return IResult::Error(nom::Err::Position(ErrorKind::Custom(11), input)),
    };
    // try to get scope vars
    let (offset, vars) = take_vars(input);
    // check for inner scopes, fail if unbalanced
    let (rest, rest_l, rest_r, is_endnode) = match take_rest_scope(offset, input) {
        Err(err) => return IResult::Error(err),
        Ok((rest, rest_l, rest_r, is_endnode)) => (rest, rest_l, rest_r, is_endnode),
    };
    // try to get terminal nodes from input[0..], check if it's lhs
    let subnodes: IResult<&[u8], Vec<ASTNode>> = many1!(rest, expand_side);
    if let IResult::Done(r, mut d) = subnodes {
        // lhs node
        if !is_endnode {
            let subnodes_r: IResult<&[u8], Vec<ASTNode>> = many1!(rest_r, expand_side);
            if subnodes_r.is_done() {
                return IResult::Error(nom::Err::Position(ErrorKind::Custom(15), rest_r));
            } else {
                // chek if the rhs is a multi decl or legal end node
                let out1 = logic_operator(rest_r);
                if out1.is_done() {
                    let (i2, op) = out1.unwrap();
                    let mut lhs = vec![];
                    for e in d {
                        lhs.push(Next::ASTNode(Box::new(e)))
                    }
                    let lhs = Next::ASTNode(Box::new(ASTNode {
                        next: Next::Chain(lhs),
                        logic_op: Some(LogicOperator::from_bytes(op)),
                        vars: None,
                    }));
                    let (r, rhs) = match scope(i2) {
                        IResult::Done(r, next) => (r, next),
                        IResult::Error(err) => return IResult::Error(err),
                        IResult::Incomplete(err) => return IResult::Incomplete(err),
                    };
                    return IResult::Done(r, Next::Chain(vec![lhs, rhs]));
                }
            }
        }
        if d.len() > 1 {
            // multiple declarations, cannot have inner scopes
            let rest_l = remove_multispace(rest_l);
            if rest_l.len() > 0 {
                return IResult::Error(nom::Err::Position(ErrorKind::Custom(15), rest_l));
            }
            let mut v0 = Vec::new();
            for e in d.drain(..) {
                v0.push(Next::ASTNode(Box::new(e)))
            }
            return IResult::Done(r, Next::Chain(v0));
        } else {
            // single decl, test for rest
            let decl = d.pop().unwrap();
            let rest_l = remove_multispace(rest_l);
            if rest_l.len() > 0 {
                match scope(rest_l) {
                    IResult::Done(r, next) => {
                        let d = Next::Chain(vec![Next::ASTNode(Box::new(decl)), next]);
                        return IResult::Done(r, d);
                    }
                    IResult::Error(err) => return IResult::Error(err),
                    IResult::Incomplete(err) => return IResult::Incomplete(err),
                }
            } else {
                return IResult::Done(r, Next::ASTNode(Box::new(decl)));
            }
        }
    }
    // parsing from beginning failed... check if it's a rhs
    // try to get terminal nodes from input[pcd..] // pcd = previous closing delimiter
    let subnodes: IResult<&[u8], Vec<ASTNode>> = many1!(rest_r, expand_side);
    if let IResult::Done(r0, mut d) = subnodes {
        // rhs node
        if d.len() > 1 {
            // multiple declarations, cannot have inner scopes
            let trial = scope(rest_l);
            if trial.is_done() {
                return IResult::Error(nom::Err::Position(ErrorKind::Custom(15), rest_l));
            }
            let mut v0 = Vec::new();
            for e in d.drain(..) {
                v0.push(Next::ASTNode(Box::new(e)))
            }
            return IResult::Done(r0, Next::Chain(v0));
        } else {
            let decl = d.pop().unwrap();
            // single decl, test for lhs
            let (r1, next) = match scope(rest_l) {
                IResult::Done(r, d) => (r, Next::Chain(vec![d, Next::ASTNode(Box::new(decl))])),
                IResult::Error(nom::Err::Position(ErrorKind::Custom(11), _)) => {
                    (r0, Next::ASTNode(Box::new(decl)))
                }
                IResult::Error(err) => return IResult::Error(err),
                IResult::Incomplete(err) => return IResult::Incomplete(err),
            };
            return IResult::Done(r1, next);
        }
    }
    // not a terminal node, keep going
    let rest = remove_multispace(rest);
    if rest.len() == 0 {
        // empty scope, will ignore further down
        return IResult::Done(rest,
                             Next::ASTNode(Box::new(ASTNode {
                                 next: Next::None,
                                 vars: None,
                                 logic_op: None,
                             })));
    }
    let (rest, next) = match scope(rest_l) {
        IResult::Done(r, n) => (r, n),
        IResult::Error(err) => return IResult::Error(err),
        _ => return IResult::Error(nom::Err::Position(ErrorKind::Complete, input)),
    };
    IResult::Done(rest,
                  Next::ASTNode(Box::new(ASTNode {
                      next: next,
                      vars: vars,
                      logic_op: None,
                  })))
}

#[allow(type_complexity)]
fn take_rest_scope(offset: usize,
                   input: &[u8])
                   -> Result<(&[u8], &[u8], &[u8], bool), nom::Err<&[u8]>> {
    let mut lp = 0;
    let mut rp = 0;
    let mut cd = 0;
    let mut nod = 0;
    let mut pcd = 0;
    let mut in_func = false;
    for (i, c) in input.iter().enumerate() {
        if *c == b'(' {
            if i > 0 && is_term_char(input[i - 1]) {
                in_func = true;
                continue;
            }
            lp += 1;
            if lp == 1 {
                nod = i;
            }
        }
        if *c == b')' && !in_func {
            rp += 1;
            pcd = cd;
            cd = i;
        } else if *c == b')' && in_func {
            in_func = false;
        }
    }
    let is_endnode;
    if offset > 0 {
        // scope has var decl
        lp -= 1;
        rp -= 1;
    }
    if (lp == 0 && rp == 0) | (lp == 0 && rp == 1) {
        is_endnode = true;
    } else {
        is_endnode = false;
    }
    // if there were vars, offset input slice to ignore them
    let rest_r;
    let rest_l;
    let mut rest;
    if offset > 0 {
        rest = &input[offset..cd];
        if (nod > pcd) && (pcd > offset) {
            rest = &input[offset..];
            rest_r = &input[pcd + 1..];
            rest_l = &input[offset..pcd + 1];
        } else if (nod > offset) && (offset > pcd) {
            rest = &input[0..];
            rest_r = &input[pcd + 1..];
            rest_l = &input[0..pcd + 1];
        } else if is_endnode {
            rest_r = &input[offset..cd];
            rest_l = &input[0..0];
        } else {
            if offset < pcd {
                rest_r = &input[pcd + 1..cd];
            } else if offset > cd {
                rest_r = &b" "[..]
            } else {
                rest_r = &input[offset..cd];
            }
            if offset < nod {
                rest_l = &input[nod..pcd + 1];
            } else {
                rest_l = &input[offset..pcd + 1];
            }
        }
    } else if nod > pcd {
        rest = &input[0..];
        rest_r = &input[pcd + 1..];
        rest_l = &input[0..pcd + 1];
    } else if is_endnode {
        rest = &input[0..cd];
        rest_r = &input[0..cd];
        rest_l = &input[0..0];
    } else {
        rest = &input[0..cd];
        rest_r = &input[pcd + 1..cd];
        rest_l = &input[nod..pcd + 1];
    }
    Ok((rest, rest_l, rest_r, is_endnode))
}

fn expand_side<'a>(input: &'a [u8]) -> IResult<&[u8], ASTNode<'a>> {
    fn is_end_node(input: &[u8]) -> IResult<&[u8], Next> {
        let f = func_decl(input);
        if f.is_done() {
            let (r, fun) = f.unwrap();
            return IResult::Done(r, Next::Assert(AssertBorrowed::FuncDecl(fun)));
        }
        let c = class_decl(input);
        if c.is_done() {
            let (r, cls) = c.unwrap();
            return IResult::Done(r, Next::Assert(AssertBorrowed::ClassDecl(cls)));
        }
        match f {
            IResult::Error(err) => IResult::Error(err),
            IResult::Incomplete(err) => IResult::Incomplete(err),
            _ => panic!(),
        }
    }    

    let input = remove_multispace(input);
    let out1 = logic_operator(input);
    if out1.is_done() {
        let (i2, op) = out1.unwrap();
        let out2 = is_end_node(i2);
        if out2.is_done() {
            // is 'op' + 'decl'
            let (r, decl) = out2.unwrap();
            let result = ASTNode {
                next: decl,
                logic_op: Some(LogicOperator::from_bytes(op)),
                vars: None,
            };
            return IResult::Done(r, result);
        } else {
            return IResult::Error(nom::Err::Position(ErrorKind::Alt, input));
        }
    } else {
        let out1 = is_end_node(input);
        if out1.is_done() {
            let (i2, decl) = out1.unwrap();
            let out2 = logic_operator(i2);
            if out2.is_done() {
                // is 'decl' + 'op'
                let (r, op) = out2.unwrap();
                let result = ASTNode {
                    next: decl,
                    logic_op: Some(LogicOperator::from_bytes(op)),
                    vars: None,
                };
                return IResult::Done(r, result);
            } else {
                // is 'decl'
                let result = ASTNode {
                    next: decl,
                    logic_op: None,
                    vars: None,
                };
                return IResult::Done(i2, result);
            }
        }
    }
    IResult::Error(nom::Err::Position(ErrorKind::Alt, input))
}
// END of scope disambiguation

// skol_decl = '(' 'exists' $(term[':'op_arg]),+ ')' ;
#[derive(Debug)]
pub struct SkolemBorrowed<'a> {
    pub name: TerminalBorrowed<'a>,
    pub op_arg: Option<OpArgBorrowed<'a>>,
}

named!(skolem(&[u8]) -> Vec<VarDeclBorrowed>, chain!(
    tag!("(") ~
    take_while!(is_multispace)? ~
    tag!("exists ") ~
    vars: fold_many1!(
        chain!(
            take_while!(is_multispace)? ~
            name: terminal ~
            oa: chain!(tag!(":") ~ oa: op_arg, ||{oa})? ~
            take_while!(is_multispace)? ~
            tag!(",")?,
            || { (name, oa) }
        ), Vec::new(), |mut vec: Vec<_>, (name, oa)| {
            let v = SkolemBorrowed {
                name: TerminalBorrowed::from_slice(name),
                op_arg: oa
            };
            vec.push(VarDeclBorrowed::Skolem(v));
            vec
        }
    ) ~
    take_while!(is_multispace)? ~
    tag!(")") ,
    || { vars }
));

// var_decl = '(' 'let' $(term[':'op_arg]),+ ')' ;
#[derive(Debug, PartialEq)]
pub struct VarBorrowed<'a> {
    pub name: TerminalBorrowed<'a>,
    pub op_arg: Option<OpArgBorrowed<'a>>,
}

named!(variable(&[u8]) -> Vec<VarDeclBorrowed>, chain!(
    tag!("(") ~
    take_while!(is_multispace)? ~
    tag!("let ") ~
    vars: fold_many1!(
        chain!(
            take_while!(is_multispace)? ~
            name: terminal ~
            oa: chain!(tag!(":") ~ oa: op_arg, ||{oa})? ~
            take_while!(is_multispace)? ~
            tag!(",")?,
            || { (name, oa) }
        ), Vec::new(), |mut vec: Vec<_>, (name, oa)| {
            let v = VarBorrowed {
                name: TerminalBorrowed::from_slice(name),
                op_arg: oa
            };
            vec.push(VarDeclBorrowed::Var(v));
            vec
        }
    ) ~
    take_while!(is_multispace)? ~
    tag!(")") ,
    || { vars }
));

// func_decl = 'fn::' term ['(' op_args ')'] args
// 			 | 'fn::' term '(' op_args ')' ;
#[derive(Debug, PartialEq)]
pub struct FuncDeclBorrowed<'a> {
    pub name: TerminalBorrowed<'a>,
    pub args: Option<Vec<ArgBorrowed<'a>>>,
    pub op_args: Option<Vec<OpArgBorrowed<'a>>>,
    pub variant: FuncVariants,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FuncVariants {
    Relational,
    NonRelational,
    TimeCalc,
}

impl FuncVariants {
    pub fn is_relational(&self) -> bool {
        match *self {
            FuncVariants::Relational => true,
            _ => false,
        }
    }
}

named!(func_decl(&[u8]) -> FuncDeclBorrowed,
    alt_complete!(
        chain!(
            take_while!(is_multispace)? ~
            tag!("fn::") ~
            name: map!(terminal, TerminalBorrowed::from_slice) ~
            op1: op_args? ~
            a1: args,
            || {
                FuncDeclBorrowed {
                    name: name,
                    args: Some(a1),
                    op_args: op1,
                    variant: FuncVariants::Relational
                }
            }
        ) |
        chain!(
            take_while!(is_multispace)? ~
            tag!("fn::") ~
            name: map!(terminal, TerminalBorrowed::from_slice) ~
            op1: op_args,
            || {
                FuncDeclBorrowed {
                    name: name,
                    args: None,
                    op_args: Some(op1),
                    variant: FuncVariants::NonRelational
                }
            }
        )
    )
);

// class_decl = term ['(' op_args ')'] args ;
#[derive(Debug, PartialEq)]
pub struct ClassDeclBorrowed<'a> {
    pub name: TerminalBorrowed<'a>,
    pub args: Vec<ArgBorrowed<'a>>,
    pub op_args: Option<Vec<OpArgBorrowed<'a>>>,
}

named!(class_decl(&[u8]) -> ClassDeclBorrowed, chain!(
    name: map!(terminal, TerminalBorrowed::from_slice) ~
    op1: op_args? ~
    a1: args ,
    || { ClassDeclBorrowed{name: name, op_args: op1, args: a1} }
));

// arg	= term [',' uval] ;
#[derive(Debug, PartialEq)]
pub struct ArgBorrowed<'a> {
    pub term: TerminalBorrowed<'a>,
    pub uval: Option<UVal>,
}

named!(arg <ArgBorrowed>, chain!(
    take_while!(is_multispace)? ~
    term: map!(terminal, TerminalBorrowed::from_slice) ~
    u0: chain!(
        take_while!(is_multispace)? ~
        char!(',') ~
        take_while!(is_multispace)? ~
        u1: uval ,
        ||{u1}
    )? ~
    take_while!(is_multispace)? ,
    || { ArgBorrowed{term: term, uval: u0} }
));

// args	= '[' arg $(arg);* ']';
named!(args(&[u8]) -> Vec<ArgBorrowed>, delimited!(
        char!('['),
        alt!(separated_list!(char!(';'), arg) | map!(arg, to_arg_vec)),
        char!(']')
    )
);

fn to_arg_vec(a: ArgBorrowed) -> Vec<ArgBorrowed> {
    vec![a]
}

// op_arg =	(string|term) [comp_op (string|term)] ;
#[derive(Debug, PartialEq)]
pub struct OpArgBorrowed<'a> {
    pub term: OpArgTermBorrowed<'a>,
    pub comp: Option<(CompOperator, OpArgTermBorrowed<'a>)>,
}

named!(op_arg <OpArgBorrowed>, chain!(
    take_while!(is_multispace)? ~
    term: alt!(
        map!(string, OpArgTermBorrowed::is_string) |
        map!(terminal, OpArgTermBorrowed::is_terminal )
    ) ~
    c1: chain!(
        take_while!(is_multispace)? ~
        c2: map!(one_of!("=<>"), CompOperator::from_char) ~
        take_while!(is_multispace)? ~
        term: alt!(
            map!(string, OpArgTermBorrowed::is_string) |
            map!(terminal, OpArgTermBorrowed::is_terminal )
        ) ~
        take_while!(is_multispace)? ,
        || { (c2, term) }
    )? ,
    || { OpArgBorrowed{term: term, comp: c1} }
));

// op_args = $(op_arg),+ ;
named!(op_args(&[u8]) -> Vec<OpArgBorrowed>, delimited!(
        char!('('),
        alt!(separated_list!(char!(','), op_arg) | map!(op_arg, to_op_arg_vec)),
        char!(')')
    )
);

fn to_op_arg_vec(a: OpArgBorrowed) -> Vec<OpArgBorrowed> {
    vec![a]
}

#[derive(Debug, PartialEq)]
pub enum OpArgTermBorrowed<'a> {
    Terminal(&'a [u8]),
    String(&'a [u8]),
}

impl<'a> OpArgTermBorrowed<'a> {
    fn is_string(i: &'a [u8]) -> OpArgTermBorrowed {
        OpArgTermBorrowed::String(i)
    }

    fn is_terminal(i: &'a [u8]) -> OpArgTermBorrowed {
        OpArgTermBorrowed::Terminal(i)
    }
}

// uval = 'u' comp_op number;
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct UVal {
    pub op: CompOperator,
    pub val: Number,
}

named!(uval <UVal>, chain!(
    take_while!(is_multispace)? ~
    char!('u') ~
    take_while!(is_multispace)? ~
    op: map!(
        one_of!("=<>"),
        CompOperator::from_char
    ) ~
    take_while!(is_multispace)? ~
    val: number ,
    || { UVal{op: op, val: val} }
));

// number = -?[0-9\.]+
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Number {
    SignedFloat(f32),
    UnsignedFloat(f32),
    SignedInteger(i32),
    UnsignedInteger(u32),
}

fn number(input: &[u8]) -> IResult<&[u8], Number> {
    let mut float = false;
    let mut idx = 0_usize;
    let rest = if (input[0] == b'-') | (input[0] == b'+') {
        &input[1..]
    } else {
        input
    };
    for (x, c) in rest.iter().enumerate() {
        if is_digit(*c) | (*c == b'.') {
            if *c == b'.' {
                float = true;
            }
            idx = x + 1;
        } else if idx > 0 {
            break;
        } else {
            return IResult::Error(nom::Err::Position(ErrorKind::Custom(1), input));
        }
    }
    if float && (input[0] == b'-') {
        IResult::Done(&input[idx + 1..],
                      Number::SignedFloat(<f32>::from_str(str::from_utf8(&input[0..idx + 1])
                              .unwrap())
                          .unwrap()))
    } else if !float && (input[0] == b'-') {
        IResult::Done(&input[idx + 1..],
                      Number::SignedInteger(<i32>::from_str(str::from_utf8(&input[0..idx + 1])
                              .unwrap())
                          .unwrap()))
    } else if float {
        IResult::Done(&input[idx..],
                      Number::UnsignedFloat(<f32>::from_str(str::from_utf8(&input[0..idx])
                              .unwrap())
                          .unwrap()))
    } else {
        IResult::Done(&input[idx..],
                      Number::UnsignedInteger(<u32>::from_str(str::from_utf8(&input[0..idx])
                              .unwrap())
                          .unwrap()))
    }
}

// string = (\".*?\")|('.*?) ;
fn string(input: &[u8]) -> IResult<&[u8], &[u8]> {
    if input[0] == b'\'' {
        delimited!(input, char!('\''), is_not!("'"), char!('\''))
    } else if input[0] == b'"' {
        delimited!(input, char!('"'), is_not!("\""), char!('"'))
    } else {
        IResult::Error(nom::Err::Position(ErrorKind::IsNotStr, input))
    }
}

// terminal = [a-zA-Z0-9_]+ ;
#[derive(PartialEq)]
pub struct TerminalBorrowed<'a>(pub &'a [u8]);

impl<'a> TerminalBorrowed<'a> {
    pub fn from_slice(i: &'a [u8]) -> TerminalBorrowed<'a> {
        TerminalBorrowed(i)
    }
}

impl<'a> ::std::fmt::Debug for TerminalBorrowed<'a> {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "Term({})", unsafe { str::from_utf8_unchecked(self.0) })
    }
}

fn terminal(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let mut idx = 0_usize;
    for (x, c) in input.iter().enumerate() {
        if is_alphanumeric(*c) | (*c == b'_') | ((*c == b'$') & (x == 0)) {
            idx = x + 1;
        } else if idx > 0 {
            break;
        } else {
            return IResult::Error(nom::Err::Position(ErrorKind::Custom(0), input));
        }
    }
    IResult::Done(&input[idx..], &input[0..idx])
}

#[allow(needless_bool)]
fn is_term_char(c: u8) -> bool {
    if is_alphanumeric(c) | (c == b'_') | (c == b'$') {
        true
    } else {
        false
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum CompOperator {
    Equal,
    Less,
    More,
}

impl CompOperator {
    fn from_char(c: char) -> CompOperator {
        if c == '<' {
            CompOperator::Less
        } else if c == '>' {
            CompOperator::More
        } else {
            CompOperator::Equal
        }
    }

    pub fn is_equal(&self) -> bool {
        match *self {
            CompOperator::Equal => true,
            _ => false,
        }
    }

    pub fn is_more(&self) -> bool {
        match *self {
            CompOperator::More => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LogicOperator {
    ICond,
    And,
    Or,
    Implication,
    Biconditional,
}

impl LogicOperator {
    fn from_bytes(m: &[u8]) -> LogicOperator {
        if m == ICOND_OP {
            LogicOperator::ICond
        } else if m == AND_OP {
            LogicOperator::And
        } else if m == OR_OP {
            LogicOperator::Or
        } else if m == IFF_OP {
            LogicOperator::Biconditional
        } else if m == IMPL_OP {
            LogicOperator::Implication
        } else {
            panic!()
        }
    }

    pub fn is_and(&self) -> bool {
        match *self {
            LogicOperator::And => true,
            _ => false,
        }
    }
}

named!(logic_operator, chain!(
    take_while!(is_multispace)? ~
    op: alt!(tag!("|>") | tag!("&&") | tag!("||") | tag!("=>") | tag!("<=>")) ~
    take_while!(is_multispace)? ,
    || { op }
));

// comment parsing tools:
named!(remove_comments(&[u8]) -> Vec<&[u8]>,
    many1!(
        chain!(
            before: comment_tag ~
            alt!(
                recognize!(delimited!(char!('#'), is_not!("\n"), alt!(is_a!("\n") | eof ))) |
                recognize!(delimited!(tag!("/*"), take_until!("*/"), tag!("*/")))
            )? ,
            || {
                before
             }
        )
    )
);

#[inline]
fn eof(input: &[u8]) -> IResult<&[u8], &[u8]> {
    if input.len() > 0 {
        IResult::Done(input, input)
    } else {
        IResult::Error(nom::Err::Position(ErrorKind::Eof, input))
    }
}

fn comment_tag(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let mut comment = false;
    let mut idx = 0_usize;
    for (i, c) in input.iter().enumerate() {
        if *c == b'#' {
            idx = i;
            break;
        } else if *c == b'/' {
            comment = true;
        } else if comment {
            if *c != b'*' {
                comment = false;
            } else {
                idx = i - 1;
                break;
            }
        }
    }
    if idx == 0 {
        IResult::Done(&b" "[..], &input[0..])
    } else {
        IResult::Done(&input[idx..], &input[0..idx])
    }
}

// white spaces and newlines parsing tools:
fn remove_multispace(input: &[u8]) -> &[u8] {
    let trimmed = take_while!(input, is_multispace);
    match trimmed {
        IResult::Done(r, _) => r,
        _ => input,
    }
}

fn is_multispace(chr: u8) -> bool {
    chr == b' ' || chr == b'\t' || chr == b'\r' || chr == b'\n'
}

#[cfg(test)]
mod test {
    use super::{class_decl, func_decl};
    use super::*;
    use std::str;

    use nom::IResult;
    use nom;

    #[test]
    fn parser_ast_output() {
        // remove comments:
        let source = b"
            # one line comment
            ( # first scope
                ( # second scope
                    (let x, y)
                    professor[$Lucy, u=1]
                )
            )
            /*
                multi line
                comment
            */
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_ok());

        // split per scopes and declarations
        let source = b"
            ( american[x,u=1] && ( weapon[y,u=1] && hostile[z,u=1 ) )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_ok());

        let source = b"
            ( american[x,u=1] && hostile[z,u=1] && ( weapon[y,u=1]) )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_err());

        let source = b"
            ( ( american[x,u=1] && hostile[z,u=1 ) && fn::criticize(t=\"now\")[$John,u=1;$Lucy] )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_ok());

        let source = b"
            ( ( american[x,u=1] ) && fn::criticize(t=\"now\")[$John,u=1;$Lucy] && weapon[y,u=1] )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_err());

        let source = b"
            ( ( ( american[x,u=1] ) ) && hostile[z,u=1] && ( ( weapon[y,u=1] ) ) )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        match scanned.unwrap_err() {
            ParseErrB::IllegalChain(_) => {}
            _ => panic!(),
        }

        let source = b"
            ( american[x,u=1] && ( ( hostile[z,u=1] ) ) && weapon[y,u=1] )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        match scanned.unwrap_err() {
            ParseErrB::IllegalChain(_) => {}
            _ => panic!(),
        }

        let source = b"
            (   ( let x y z )
                (
                    ( american[x,u=1] && weapon[y,u=1] && fn::sells[y,u>0.5;x;z] && hostile[z,u=1] )
                    |> criminal[x,u=1]
                )
            )
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_ok());
        let s0 = scanned.unwrap().pop().unwrap();
        let s0 = match s0 {
            Next::ASTNode(val) => *val,
            _ => panic!(),
        };
        assert!(s0.vars.is_some());
        match s0.next {
            Next::Chain(s1) => {
                assert_eq!(s1.len(), 2);
                match s1[0] {
                    Next::Chain(ref s2_0) => {
                        assert_eq!(s2_0.len(), 4);
                    }
                    _ => panic!(),
                }
                match s1[1] {
                    Next::ASTNode(ref s2_1) => {
                        assert_eq!(s2_1.logic_op.as_ref().unwrap(),
                                   &LogicOperator::ICond);
                        match s2_1.next {
                            Next::Assert(AssertBorrowed::ClassDecl(_)) => {}
                            _ => panic!(),
                        }
                    }
                    _ => panic!(),
                };
            }
            _ => panic!(),
        }

        let source = b"
        ((let x y) (american[x,u=1] && hostile[z,u=1]) |> criminal[x,u=1])
        ((let x y) ((american[x,u=1] && hostile[z,u=1]) |> criminal[x,u=1]))
        ((let x y) (american[x,u=1] && hostile[z,u=1]) |> criminal[x,u=1])
        ";
        let mut data = Vec::new();
        let scanned = Parser::feed(source, &mut data);
        assert!(scanned.is_ok());
        assert_eq!(scanned.unwrap().len(), 3);
    }

    macro_rules! assert_done_or_err {
        ($i:ident) => {{
            match $i {
                IResult::Error(nom::Err::Position(ref t, ref v)) => {
                    println!("\n@error Err::{:?}: {:?}", t, unsafe{str::from_utf8_unchecked(v)});
                },
                _ => {}
            }
            assert!(!$i.is_err());
        }}
    }

    #[test]
    fn parser_predicate() {
        let s1 = b"professor[$Lucy,u=1]";
        let s1_res = class_decl(s1);
        assert_done_or_err!(s1_res);
        let s1_res = s1_res.unwrap().1;
        assert_eq!(s1_res.name, TerminalBorrowed(b"professor"));
        assert_eq!(s1_res.args[0].term, TerminalBorrowed(b"$Lucy"));
        assert!(s1_res.args[0].uval.is_some());

        let s2 = b"missile[$M1,u>-1.5]";
        let s2_res = class_decl(s2);
        assert_done_or_err!(s2_res);
        let s2_res = s2_res.unwrap().1;
        assert_eq!(s2_res.name, TerminalBorrowed(b"missile"));
        assert_eq!(s2_res.args[0].term, TerminalBorrowed(b"$M1"));
        let s2_uval = s2_res.args[0].uval.as_ref().unwrap();
        assert_eq!(s2_uval.op, CompOperator::More);
        assert_eq!(s2_uval.val, Number::SignedFloat(-1.5_f32));

        let s3 = b"dean(t1=\"now\",t2=t1)[$John,u=0]";
        let s3_res = class_decl(s3);
        assert_done_or_err!(s3_res);
        let s3_res = s3_res.unwrap().1;
        assert_eq!(s3_res.name, TerminalBorrowed(b"dean"));
        assert_eq!(s3_res.args[0].term, TerminalBorrowed(b"$John"));
        assert!(s3_res.args[0].uval.is_some());
        assert_eq!(
            s3_res.op_args.as_ref().unwrap(),
            &vec![OpArgBorrowed{term: OpArgTermBorrowed::Terminal(b"t1"),
                        comp: Some((CompOperator::Equal, OpArgTermBorrowed::String(b"now")))},
                  OpArgBorrowed{term: OpArgTermBorrowed::Terminal(b"t2"),
                        comp: Some((CompOperator::Equal, OpArgTermBorrowed::Terminal(b"t1")))}]
        );

        let s4 = b"animal(t=\"2015.07.05.11.28\")[cow, u=1; brown, u=0.5]";
        let s4_res = class_decl(s4);
        assert_done_or_err!(s4_res);
        let s4_res = s4_res.unwrap().1;
        assert_eq!(s4_res.args[1].term, TerminalBorrowed(b"brown"));
        assert!(s4_res.op_args.is_some());
        assert_eq!(s4_res.op_args.as_ref().unwrap(),
            &vec![OpArgBorrowed{term: OpArgTermBorrowed::Terminal(b"t"),
                        comp: Some((CompOperator::Equal,
                                    OpArgTermBorrowed::String(b"2015.07.05.11.28")))}]);
    }

    #[test]
    fn parser_function() {
        let s1 = b"fn::criticize(t=\"now\")[$John,u=1;$Lucy]";
        let s1_res = func_decl(s1);
        assert_done_or_err!(s1_res);
        assert_eq!(s1_res.unwrap().1.variant, FuncVariants::Relational);

        let s2 = b"fn::takes[$analysis,u>0;$Bill]";
        let s2_res = func_decl(s2);
        assert_done_or_err!(s2_res);
        let s2_res = s2_res.unwrap().1;
        assert_eq!(s2_res.name, TerminalBorrowed(b"takes"));
        assert_eq!(s2_res.variant, FuncVariants::Relational);

        let s3 = b"fn::loves[cow, u=1; bull ]";
        let s3_res = func_decl(s3);
        assert_done_or_err!(s3_res);
        assert_eq!(s3_res.unwrap().1.variant, FuncVariants::Relational);

        let s4 = b"fn::time_calc(t1<t2)";
        let s4_res = func_decl(s4);
        assert_done_or_err!(s4_res);
        assert_eq!(s4_res.unwrap().1.variant, FuncVariants::NonRelational);
    }
}
