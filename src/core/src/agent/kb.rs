//! Main knowledge-base logic module, in this module reside the different
//! types that transform and store the data for the individual agents and
//! serve as representations of the different objects and the relationships
//! between them.
//!
//! Main
//! ----
//! `Representation`: Main type, stores all the representations and
//! relationships for a given agent in a concrete time.
//!
//! `Entity`: Represents a singular entity, which is the unique
//! member of it's own set.
//!
//! `Classes`: The sets in which the agent can classify objects.
//! Also stores the types of relations an object can have.
//!
//! Support types, methods and functions
//! -------------------------------------------
//! `Inference`: Encapsulates the whole inference process, from making
//! a temporal substitution representation where the inference is operated to
//! solving the query (including query parsing, data fetching and unification).

use std::collections::{HashMap, VecDeque};

use lang;
use lang::{ParseTree, ParseErrF, GroundedTerm};

pub struct Representation {
    entities: HashMap<usize, usize>,
    classes: HashMap<usize, usize>,
}

/// This type is a container for internal agent's representations.
/// An agent can have any number of such representations at any moment,
/// all of which are contained in this object.
///
/// The class includes methods to encode and decode the representations
/// to/from data streams or idioms.
///
/// Attributes:
///     entities -> Unique members (entities) of their own set/class.
///     | Entities are denoted with a $ symbol followed by a name.
///     classes -> Sets of objects that share a common property.
impl Representation {
    pub fn new() -> Representation {
        Representation {
            entities: HashMap::new(),
            classes: HashMap::new(),
        }
    }

    /// Parses a sentence (or several of them) into an usable formula
    /// and stores it into the internal representation along with the
    /// corresponding classes. In case the sentence is a predicate,
    /// the objects get declared as members of their classes.
    ///
    /// Accepts first-order logic sentences sentences, both atomic
    /// sentences ('Lucy is a professor') and complex sentences compossed
    /// of different atoms and operators ('If someone is a professor,
    /// then it's a person'). Examples:
    ///
    /// '''>>> r.tell("(professor[$Lucy,u=1])")'''
    /// will include the individual '$Lucy' in the professor category)
    /// '''>>> r.tell("((let x) professor[x,u=1] |> person[x,u=1])")'''
    /// all the individuals which are professors will be added to the
    /// person category, and the formula will be stored in the professor
    /// class for future use.
    ///
    /// For more examples check the LogSentence type docs.
    pub fn tell(&mut self, source: String) -> Result<(), Vec<ParseErrF>> {
        let pres = lang::logic_parser(source);
        if pres.is_ok() {
            let mut pres: VecDeque<ParseTree> = pres.unwrap();
            let mut errors = Vec::new();
            for _ in 0..pres.len() {
                match pres.pop_front().unwrap() {
                    ParseTree::Assertion(assertions) => {
                        for assertion in assertions {
                            if assertion.is_class() {
                                self.up_membership(assertion.unwrap_cls())
                            } else {
                                self.up_relation(assertion.unwrap_fn())
                            }
                        }
                    }
                    ParseTree::IExpr(iexpr) => self.add_belief(iexpr),
                    ParseTree::Rule(rule) => self.add_rule(rule),
                    ParseTree::ParseErr(err) => errors.push(err),
                }
            }
            if errors.len() > 0 {
                Err(errors)
            } else {
                Ok(())
            }
        } else {
            Err(vec![pres.unwrap_err()])
        }
    }

    /// Asks the KB if some fact is true and returns the answer to the query.
    pub fn ask(&mut self, source: String, single_answer: bool) -> Answer {
        let pres = lang::logic_parser(source);
        if pres.is_ok() {
            let pres = pres.unwrap();
            Inference::new(pres, single_answer)
        } else {
            Answer::ParseErr(pres.unwrap_err())
        }
    }

    fn up_membership(&mut self, assert: lang::ClassDecl) {}

    fn up_relation(&mut self, assert: lang::FuncDecl) {}

    fn add_belief(&mut self, belief: lang::LogSentence) {}

    fn add_rule(&mut self, rule: lang::LogSentence) {}

    pub fn get_entity_from_class(&self, name: &str) -> Option<&GroundedTerm> {

    }

    pub fn test_predicates(&self,
                           req: &HashMap<*const lang::Var, Vec<*const lang::Assert>>)
                           -> Option<&HashMap<*const lang::Var, Vec<&GroundedTerm>>> {
        // stub
        panic!()
    }
}

pub enum Answer {
    Single(Option<bool>),
    Multiple(Vec<Option<bool>>),
    QueryErr,
    ParseErr(ParseErrF),
}

struct Inference {
    results: Vec<Option<bool>>,
}

impl Inference {
    fn new(query: VecDeque<ParseTree>, single: bool) -> Answer {
        let inf = Inference { results: Vec::new() };
        if single {
            for r in inf.get_results() {
                if r.is_some() {
                    match r {
                        Some(false) => return Answer::Single(Some(false)),
                        _ => {}
                    }
                } else {
                    return Answer::Single(None);
                }
            }
            Answer::Single(Some(true))
        } else {
            Answer::Multiple(inf.get_results())
        }
    }

    fn get_results(self) -> Vec<Option<bool>> {
        let Inference { results } = self;
        results
    }
}
