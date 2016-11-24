//! Main knowledge-base logic module, in this module reside the different
//! types that transform and store the data for the individual agents and
//! serve as representations of the different objects and the relationships
//! between them.
//!
//! ## Safety
//! There is some unsafe code on this module, the unsafe code performs to taks:
//!     * Assure the compiler that data being referenced will live long enough,
//!       this is, increasing the lifetime bounds.
//!     * Bypass the ```Send``` trait type check when multithreading.
//!
//! Both of those are safe because a Representation owns, uniquely, all its
//! knowledge by the duration of it's own lifetime (data is never dropped, while
//! the representation is alive), thereby is safe to point to the data being
//! referenced from the representation or the query (for the duration of the query).
//!
//! There is an intensive use of reference counted data throught the module,
//! to avoid unnecesary copies. The problem is that Rc is not ```Send``` compliant
//! so we need to 'hack' the type checking system through unsafe code to cheat
//! the compiler.

use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::FromIterator;
use std::hash::{Hash, Hasher};
use std::sync::{RwLock, Mutex};
use std::rc::Rc;

use chrono::{UTC, DateTime};
use scoped_threadpool::Pool;

use lang;
use lang::{ParseTree, ParseErrF, GroundedClsMemb, GroundedFunc, LogSentence};

type Date = DateTime<UTC>;

pub struct Representation {
    entities: RwLock<HashMap<Rc<String>, Box<Entity>>>,
    classes: RwLock<HashMap<Rc<String>, Box<Class>>>,
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
///     | Entities are denoted with a $ symbol followed by an id.
///     classes -> Sets of objects (entities or subclasses) that share a common property.
///     | This includes 'classes of relationships' and other 'functions'.
impl Representation {
    pub fn new() -> Representation {
        Representation {
            entities: RwLock::new(HashMap::new()),
            classes: RwLock::new(HashMap::new()),
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
    /// `>>> r.tell("(professor[$Lucy,u=1])")`
    /// will include the individual '$Lucy' in the professor category)
    /// `>>> r.tell("((let x) professor[x,u=1] |> person[x,u=1])")`
    /// all the individuals which are professors will be added to the
    /// person category, and the formula will be stored in the professor
    /// class for future use.
    ///
    /// For more examples check the LogSentence type docs.
    pub fn tell(&self, source: String) -> Result<(), Vec<ParseErrF>> {
        let pres = lang::logic_parser(source, true);
        if pres.is_ok() {
            let mut pres: VecDeque<ParseTree> = pres.unwrap();
            let mut errors = Vec::new();
            for _ in 0..pres.len() {
                match pres.pop_front().unwrap() {
                    ParseTree::Assertion(assertions) => {
                        for assertion in assertions {
                            if assertion.is_class() {
                                for a in assertion.unwrap_cls().into_iter() {
                                    self.up_membership(Rc::new(a))
                                }
                            } else {
                                let a = Rc::new(assertion.unwrap_fn().into_grounded());
                                self.up_relation(a)
                            }
                        }
                    }
                    ParseTree::IExpr(iexpr) => self.add_belief(Rc::new(iexpr)),
                    ParseTree::Expr(rule) => self.add_rule(Rc::new(rule)),
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
    pub fn ask(&self, source: String) -> Answer {
        let pres = lang::logic_parser(source, false);
        if pres.is_ok() {
            let pres = QueryInput::ManyQueries(pres.unwrap());
            let mut inf = match Inference::new(&self, pres, false) {
                Ok(inf) => inf,
                Err(()) => return Answer::QueryErr,
            };
            {
                let mut inf_r = unsafe { &mut *(&mut inf as *mut Box<Inference>) };
                inf_r.infer_facts();
            }
            inf.get_results()
        } else {
            Answer::ParseErr(pres.unwrap_err())
        }
    }

    fn ask_processed(&self, source: QueryInput, ignore_current: bool) -> Answer {
        let mut inf = match Inference::new(&self, source, ignore_current) {
            Ok(inf) => inf,
            Err(()) => return Answer::QueryErr,
        };
        {
            let mut inf_r = unsafe { &mut *(&mut inf as *mut Box<Inference>) };
            inf_r.infer_facts();
        }
        inf.get_results()
    }

    pub fn up_membership(&self, assert: Rc<lang::GroundedClsMemb>) {
        let parent_exists = self.classes.read().unwrap().contains_key(&assert.get_parent());
        if !parent_exists {
            let class = Box::new(Class::new(assert.get_parent(), ClassKind::Membership));
            self.classes.write().unwrap().insert(class.name.clone(), class);
        }
        let decl;
        let is_new: bool;
        match (&assert.get_name()).starts_with("$") {
            true => {
                let entity_exists = self.entities.read().unwrap().contains_key(&assert.get_name());
                if entity_exists {
                    let lock = self.entities.read().unwrap();
                    let entity = lock.get(&assert.get_name()).unwrap();
                    is_new = entity.add_class_membership(assert.clone());
                    decl = ClassMember::Entity(assert.clone());
                } else {
                    let entity = Box::new(Entity::new(assert.get_name()));
                    is_new = entity.add_class_membership(assert.clone());
                    self.entities.write().unwrap().insert(entity.name.clone(), entity);
                    decl = ClassMember::Entity(assert.clone());
                }
            }
            false => {
                let class_exists = self.classes.read().unwrap().contains_key(&assert.get_name());
                if class_exists {
                    let lock = self.classes.read().unwrap();
                    let class = lock.get(&assert.get_name()).unwrap();
                    is_new = class.add_class_membership(assert.clone());
                    decl = ClassMember::Class(assert.clone());
                } else {
                    let class = Box::new(Class::new(assert.get_name(), ClassKind::Membership));
                    is_new = class.add_class_membership(assert.clone());
                    self.classes.write().unwrap().insert(class.name.clone(), class);
                    decl = ClassMember::Class(assert.clone());
                }
            }
        }
        if is_new {
            let lock = self.classes.read().unwrap();
            let parent = lock.get(&assert.get_parent()).unwrap();
            parent.add_member(decl);
        }
    }

    pub fn up_relation(&self, assert: Rc<lang::GroundedFunc>) {
        // it doesn't matter this is overwritten, as if it exists, it exists for all
        let is_new = Rc::new(::std::cell::RefCell::new(true));
        let process_arg = |a: &GroundedClsMemb| {
            let subject = a.get_name();
            let is_new1;
            match (&subject).starts_with("$") {
                true => {
                    let entity_exists = self.entities.read().unwrap().contains_key(&subject);
                    if entity_exists {
                        let lock = self.entities.read().unwrap();
                        let entity = lock.get(&subject).unwrap();
                        is_new1 = entity.add_relationship(assert.clone());
                    } else {
                        let entity = Box::new(Entity::new(subject));
                        is_new1 = entity.add_relationship(assert.clone());
                        self.entities.write().unwrap().insert(entity.name.clone(), entity);
                    }
                }
                false => {
                    let class_exists = self.classes.read().unwrap().contains_key(&subject);
                    if class_exists {
                        let lock = self.classes.read().unwrap();
                        let class = lock.get(&subject).unwrap();
                        is_new1 = class.add_relationship(assert.clone());
                    } else {
                        let class = Box::new(Class::new(subject, ClassKind::Membership));
                        is_new1 = class.add_relationship(assert.clone());
                        self.classes.write().unwrap().insert(class.name.clone(), class);
                    }
                }
            }
            let new_check = is_new.clone();
            *new_check.borrow_mut() = is_new1;
        };
        let relation_exists = self.classes.read().unwrap().contains_key(&assert.name);
        if !relation_exists {
            let relationship = Box::new(Class::new(assert.name.clone(), ClassKind::Relationship));
            self.classes.write().unwrap().insert(relationship.name.clone(), relationship);
        }
        process_arg(&assert.args[0]);
        process_arg(&assert.args[1]);
        if assert.third.is_some() {
            process_arg(assert.third.as_ref().unwrap())
        }
        if *is_new.borrow() {
            let lock = self.classes.read().unwrap();
            let parent = lock.get(&assert.name).unwrap();
            parent.add_grounded_relationship(assert.clone());
        }
    }

    fn add_belief(&self, belief: Rc<LogSentence>) {
        fn update(subject: Rc<String>,
                  name: Rc<String>,
                  is_entity: bool,
                  belief: Rc<LogSentence>,
                  repr: &Representation) {
            if is_entity {
                let entity_exists = repr.entities.read().unwrap().contains_key(&subject);
                if entity_exists {
                    let lock = repr.entities.read().unwrap();
                    let entity = lock.get(&subject).unwrap();
                    entity.add_belief(belief.clone(), subject);
                } else {
                    let entity = Box::new(Entity::new(subject.clone()));
                    entity.add_belief(belief.clone(), name);
                    repr.entities
                        .write()
                        .unwrap()
                        .insert(subject.clone(), entity);
                }
            } else {
                let class_exists = repr.classes.read().unwrap().contains_key(&subject);
                if class_exists {
                    let lock = repr.classes.read().unwrap();
                    let class = lock.get(&subject).unwrap();
                    class.add_belief(belief.clone(), name);
                } else {
                    let class = Box::new(Class::new(subject.clone(), ClassKind::Membership));
                    class.add_belief(belief.clone(), name);
                    repr.classes
                        .write()
                        .unwrap()
                        .insert(subject.clone(), class);
                }
            }
        };

        for p in belief.get_all_predicates() {
            match p {
                &lang::Assert::ClassDecl(ref cls_decl) => {
                    let class_exists = self.classes
                        .read()
                        .unwrap()
                        .contains_key(&cls_decl.get_name());
                    if class_exists {
                        self.classes
                            .read()
                            .unwrap()
                            .get(&cls_decl.get_name())
                            .unwrap()
                            .add_belief(belief.clone(), cls_decl.get_name())
                    } else {
                        let class = Box::new(Class::new(cls_decl.get_name(),
                                                        ClassKind::Membership));
                        class.add_belief(belief.clone(), cls_decl.get_name());
                        self.classes.write().unwrap().insert(class.name.clone(), class);
                    }
                    for arg in cls_decl.get_args() {
                        if !arg.is_var() {
                            let subject = arg.get_name();
                            match subject.starts_with("$") {
                                true => {
                                    update(subject,
                                           cls_decl.get_name(),
                                           true,
                                           belief.clone(),
                                           &self)
                                }
                                false => {
                                    update(subject,
                                           cls_decl.get_name(),
                                           false,
                                           belief.clone(),
                                           &self)
                                }
                            }
                        }
                    }
                }
                &lang::Assert::FuncDecl(ref fn_decl) => {
                    let class_exists = self.classes
                        .read()
                        .unwrap()
                        .contains_key(&fn_decl.get_name());
                    if class_exists {
                        self.classes
                            .read()
                            .unwrap()
                            .get(&fn_decl.get_name())
                            .unwrap()
                            .add_belief(belief.clone(), fn_decl.get_name())
                    } else {
                        let class = Box::new(Class::new(fn_decl.get_name(),
                                                        ClassKind::Relationship));
                        class.add_belief(belief.clone(), fn_decl.get_name());
                        self.classes.write().unwrap().insert(class.name.clone(), class);
                    }
                    for arg in fn_decl.get_args() {
                        if !arg.is_var() {
                            let subject = arg.get_name();
                            match subject.starts_with("$") {
                                true => {
                                    update(subject, fn_decl.get_name(), true, belief.clone(), &self)
                                }
                                false => {
                                    update(subject,
                                           fn_decl.get_name(),
                                           false,
                                           belief.clone(),
                                           &self)
                                }
                            }
                        }
                    }
                }
            }
        }

        for query in belief.get_rhs_predicates() {
            match query {
                &lang::Assert::ClassDecl(ref cls_decl) => {
                    self.ask_processed(QueryInput::AskClassMember(cls_decl.clone()), true);
                }
                &lang::Assert::FuncDecl(ref func_decl) => {
                    self.ask_processed(QueryInput::AskRelationalFunc(func_decl.clone()), true);
                }
            }
        }
    }

    fn add_rule(&self, rule: Rc<LogSentence>) {
        let mut n = Vec::new();
        let preds = rule.get_all_predicates();
        for p in preds {
            let name = p.get_name();
            n.push(name.clone());
            let class_exists = self.classes.read().unwrap().contains_key(&name);
            if class_exists {
                let lock = self.classes.read().unwrap();
                let class = lock.get(&name).unwrap();
                class.add_rule(rule.clone());
            } else {
                let nc = match p {
                    &lang::Assert::ClassDecl(_) => {
                        Box::new(Class::new(name.clone(), ClassKind::Membership))
                    }
                    &lang::Assert::FuncDecl(_) => {
                        Box::new(Class::new(name.clone(), ClassKind::Relationship))
                    }
                };
                nc.add_rule(rule.clone());
                self.classes.write().unwrap().insert(name, nc);
            }
        }
        let obj_dic = self.by_class(&n);
        for _e in obj_dic {
            panic!("not implemented: rule.call(self, e)")
        }
    }

    fn by_class(&self, classes: &Vec<Rc<String>>) -> HashMap<Rc<String>, Vec<Rc<GroundedClsMemb>>> {
        let mut dict = HashMap::new();
        let lock = self.classes.read().unwrap();
        for cls in classes {
            let cls_ref = lock.get(cls);
            if cls_ref.is_none() {
                continue;
            }
            let mut v = vec![];
            for e in &**cls_ref.unwrap().members.read().unwrap() {
                match *e {
                    ClassMember::Class(ref m) => v.push(m.clone()),
                    ClassMember::Entity(ref m) => v.push(m.clone()),
                    _ => {}
                }
            }
            dict.insert(cls.clone(), v);
        }
        dict
    }

    fn by_relationship(&self,
                       funcs: &Vec<&lang::FuncDecl>)
                       -> HashMap<Rc<String>, HashMap<Rc<String>, Vec<Rc<GroundedFunc>>>> {
        let mut dict = HashMap::new();
        let lock = self.classes.read().unwrap();
        for func in funcs {
            let func_ref = lock.get(&func.get_name());
            if func_ref.is_none() {
                continue;
            }
            let mut m = HashMap::new();
            for e in &**func_ref.unwrap().members.read().unwrap() {
                match *e {
                    ClassMember::Func(ref f) => {
                        for name in f.get_args_names() {
                            let e: &mut Vec<_> = m.entry(name).or_insert(Vec::new());
                            e.push(f.clone())
                        }
                    }
                    _ => {}
                }
            }
            dict.insert(func.get_name(), m);
        }
        dict
    }

    pub fn get_entity_from_class(&self,
                                 class: Rc<String>,
                                 subject: Rc<String>)
                                 -> Option<Rc<GroundedClsMemb>> {
        match subject.starts_with("$") {
            true => {
                let entity_exists = self.entities.read().unwrap().contains_key(&subject);
                if entity_exists {
                    let lock = self.entities.read().unwrap();
                    match lock.get(&subject).unwrap().belongs_to_class(class) {
                        Some(r) => Some(r.clone()),
                        None => None,
                    }
                } else {
                    None
                }
            }
            false => {
                let class_exists = self.classes.read().unwrap().contains_key(&subject);
                if class_exists {
                    let lock = self.classes.read().unwrap();
                    match lock.get(&subject).unwrap().belongs_to_class(class) {
                        Some(r) => Some(r.clone()),
                        None => None,
                    }
                } else {
                    None
                }
            }
        }
    }

    fn class_membership(&self, pred: &GroundedClsMemb) -> Option<bool> {
        let subject = pred.get_name();
        match subject.starts_with("$") {
            true => {
                if let Some(entity) = self.entities.read().unwrap().get(&subject) {
                    if let Some(current) = entity.belongs_to_class(pred.get_parent()) {
                        if *current == *pred {
                            return Some(true);
                        } else {
                            return Some(false);
                        }
                    }
                }
            }
            false => {
                if let Some(class) = self.classes.read().unwrap().get(&subject) {
                    if let Some(current) = class.belongs_to_class(pred.get_parent()) {
                        if *current == *pred {
                            return Some(true);
                        } else {
                            return Some(false);
                        }
                    }
                }
            }
        }
        None
    }

    fn get_class_membership(&self, subject: &lang::FreeClsOwner) -> Vec<Rc<GroundedClsMemb>> {
        let ref name = subject.term;
        match name.starts_with("$") {
            true => {
                if let Some(entity) = self.entities.read().unwrap().get(name) {
                    entity.get_class_membership(subject)
                } else {
                    vec![]
                }
            }
            false => {
                if let Some(class) = self.classes.read().unwrap().get(name) {
                    class.get_class_membership(subject)
                } else {
                    vec![]
                }
            }
        }
    }

    pub fn has_relationship(&self, pred: &GroundedFunc, subject: Rc<String>) -> Option<bool> {
        match subject.starts_with("$") {
            true => {
                if let Some(entity) = self.entities.read().unwrap().get(&subject) {
                    if let Some(current) = entity.has_relationship(pred) {
                        if *current == *pred {
                            return Some(true);
                        } else {
                            return Some(false);
                        }
                    }
                }
            }
            false => {
                if let Some(class) = self.classes.read().unwrap().get(&subject) {
                    if let Some(current) = class.has_relationship(pred) {
                        if *current == *pred {
                            return Some(true);
                        } else {
                            return Some(false);
                        }
                    }
                }
            }
        }
        None
    }
}

/// An entity is the unique member of it's own class.
/// Represents an object which can pertain to multiple classes or sets.
/// It's an abstraction owned by an agent, the internal representation
/// of the object, not the object itself.
///
/// An entity inherits the properties of the classes it belongs to,
/// and has some implicit attributes which are unique to itself.
///
/// Membership to a class is denoted (following fuzzy sets) by a real number
/// between 0 and 1. If the number is one, the object will always belong to
/// the set, if it's zero, it will never belong to the set.
///
/// For example, an object can belong to the set 'cold' with a degree of
/// 0.9 (in natural language then it would be 'very cold') or 0.1
/// (then it would be 'a bit cold', the subjective adjectives are defined
/// in the class itself).
#[derive(Debug)]
pub struct Entity {
    pub name: Rc<String>,
    classes: RwLock<HashMap<Rc<String>, Rc<GroundedClsMemb>>>,
    relations: RwLock<HashMap<Rc<String>, Vec<Rc<GroundedFunc>>>>,
    beliefs: RwLock<HashMap<Rc<String>, Vec<Rc<LogSentence>>>>,
}

impl Entity {
    fn new(name: Rc<String>) -> Entity {
        Entity {
            name: name,
            classes: RwLock::new(HashMap::new()),
            relations: RwLock::new(HashMap::new()),
            beliefs: RwLock::new(HashMap::new()),
        }
    }

    fn belongs_to_class(&self, class_name: Rc<String>) -> Option<Rc<GroundedClsMemb>> {
        let lock = self.classes.read().unwrap();
        match lock.get(&class_name) {
            Some(r) => Some(r.clone()),
            None => None,
        }
    }

    fn get_class_membership(&self, compare: &lang::FreeClsOwner) -> Vec<Rc<GroundedClsMemb>> {
        let lock = self.classes.read().unwrap();
        let v: Vec<_> = lock.values().filter(|x| compare.filter_grounded(&**x)).cloned().collect();
        v
    }

    fn add_class_membership(&self, grounded: Rc<GroundedClsMemb>) -> bool {
        let mut lock = self.classes.write().unwrap();
        let name = grounded.get_parent();
        let stmt_exists = lock.contains_key(&name);
        match stmt_exists {
            true => {
                let current = lock.get(&name).unwrap();
                current.update(grounded);
                false
            }
            false => {
                lock.insert(name, grounded);
                true
            }
        }
    }

    fn has_relationship(&self, func: &GroundedFunc) -> Option<Rc<GroundedFunc>> {
        let lock = self.relations.read().unwrap();
        if let Some(relation_type) = lock.get(&func.get_name()) {
            for rel in relation_type {
                if rel.comparable(func) {
                    return Some(rel.clone());
                }
            }
        }
        None
    }

    fn add_relationship(&self, func: Rc<GroundedFunc>) -> bool {
        let mut lock = self.relations.write().unwrap();
        let name = func.get_name();
        let stmt_exists = lock.contains_key(&name);
        match stmt_exists {
            true => {
                let funcs_type = lock.get_mut(&name).unwrap();
                let mut found_rel = false;
                for f in funcs_type.iter_mut() {
                    if f.comparable(&*func) {
                        f.update(func.clone());
                        found_rel = true;
                        break;
                    }
                }
                if !found_rel {
                    funcs_type.push(func.clone());
                    true
                } else {
                    false
                }
            }
            false => {
                lock.insert(name, vec![func.clone()]);
                true
            }
        }
    }

    fn add_belief(&self, belief: Rc<LogSentence>, parent: Rc<String>) {
        let sent_exists = self.beliefs.read().unwrap().contains_key(&parent);
        match sent_exists {
            true => {
                if let Some(ls) = self.beliefs.write().unwrap().get_mut(&parent) {
                    ls.push(belief)
                }
            }
            false => {
                self.beliefs.write().unwrap().insert(parent, vec![belief]);
            }
        }
    }
}

/// A class is a set of entities that share some properties.
/// It can be a subset of others supersets, and viceversa.
///
/// Membership is not binary, but fuzzy, being the extreme cases (0, 1)
/// the classic binary membership. Likewise, membership to a class can be
/// temporal. For more info check `Entity` type documentation.
///
/// All the attributes of a class are inherited by their members
/// (to a fuzzy degree).
#[derive(Debug)]
pub struct Class {
    pub name: Rc<String>,
    classes: RwLock<HashMap<Rc<String>, Rc<GroundedClsMemb>>>,
    relations: RwLock<HashMap<Rc<String>, Vec<Rc<GroundedFunc>>>>,
    beliefs: RwLock<HashMap<Rc<String>, Vec<Rc<LogSentence>>>>,
    rules: RwLock<Vec<Rc<LogSentence>>>,
    kind: ClassKind,
    members: RwLock<Vec<ClassMember>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassKind {
    Relationship,
    Membership,
}

#[derive(Debug, Clone)]
enum ClassMember {
    Entity(Rc<GroundedClsMemb>),
    Class(Rc<GroundedClsMemb>),
    Func(Rc<GroundedFunc>),
}

impl Class {
    fn new(name: Rc<String>, kind: ClassKind) -> Class {
        Class {
            name: name,
            classes: RwLock::new(HashMap::new()),
            relations: RwLock::new(HashMap::new()),
            beliefs: RwLock::new(HashMap::new()),
            rules: RwLock::new(Vec::new()),
            kind: kind,
            members: RwLock::new(Vec::new()),
        }
    }

    fn belongs_to_class(&self, class_name: Rc<String>) -> Option<Rc<GroundedClsMemb>> {
        let lock = self.classes.read().unwrap();
        match lock.get(&class_name) {
            Some(r) => Some(r.clone()),
            None => None,
        }
    }

    fn get_class_membership(&self, compare: &lang::FreeClsOwner) -> Vec<Rc<GroundedClsMemb>> {
        let lock = self.classes.read().unwrap();
        let v: Vec<_> = lock.values().filter(|x| compare.filter_grounded(&**x)).cloned().collect();
        v
    }

    /// Set a superclass of this class
    fn add_class_membership(&self, grounded: Rc<GroundedClsMemb>) -> bool {
        let mut lock = self.classes.write().unwrap();
        let name = grounded.get_parent();
        let stmt_exists = lock.contains_key(&name);
        match stmt_exists {
            true => {
                let current = lock.get_mut(&name).unwrap();
                current.update(grounded);
                false
            }
            false => {
                lock.insert(name, grounded);
                true
            }
        }
    }

    /// Add members of this class, being them other classes or entities.
    fn add_member(&self, member: ClassMember) {
        self.members.write().unwrap().push(member);
    }

    fn has_relationship(&self, func: &GroundedFunc) -> Option<Rc<GroundedFunc>> {
        let lock = self.relations.read().unwrap();
        if let Some(relation_type) = lock.get(&func.get_name()) {
            for rel in relation_type {
                if rel.comparable(func) {
                    return Some(rel.clone());
                }
            }
        }
        None
    }

    /// Add a relationship this class has with other classes/entities
    fn add_relationship(&self, func: Rc<GroundedFunc>) -> bool {
        let mut lock = self.relations.write().unwrap();
        let name = func.get_name();
        let stmt_exists = lock.contains_key(&name);
        match stmt_exists {
            true => {
                let funcs_type = lock.get_mut(&name).unwrap();
                let mut found_rel = false;
                for f in funcs_type.iter_mut() {
                    if f.comparable(&*func) {
                        f.update(func.clone());
                        found_rel = true;
                        break;
                    }
                }
                if !found_rel {
                    funcs_type.push(func.clone());
                    true
                } else {
                    false
                }
            }
            false => {
                lock.insert(name, vec![func.clone()]);
                true
            }
        }
    }

    /// Add a grounded relationship of this kind of relationship
    fn add_grounded_relationship(&self, func: Rc<GroundedFunc>) {
        self.members.write().unwrap().push(ClassMember::Func(func));
    }

    fn add_belief(&self, belief: Rc<LogSentence>, parent: Rc<String>) {
        let sent_exists = self.beliefs.read().unwrap().contains_key(&parent);
        match sent_exists {
            true => {
                if let Some(ls) = self.beliefs.write().unwrap().get_mut(&parent) {
                    ls.push(belief)
                }
            }
            false => {
                self.beliefs.write().unwrap().insert(parent, vec![belief]);
            }
        }
    }

    fn add_rule(&self, rule: Rc<LogSentence>) {
        self.rules.write().unwrap().push(rule);
    }
}

type PArgVal = Vec<usize>;
struct Inference<'a> {
    query: QueryProcessed<'a>,
    kb: &'a Representation,
    ignore_current: bool,
    nodes: RwLock<HashMap<Rc<String>, Vec<Box<ProofNode>>>>,
    queue: RwLock<HashMap<*const ProofNode, HashSet<PArgVal>>>,
    results: InfResults<'a>,
    repeat: Mutex<Vec<(Rc<LogSentence>, PArgVal)>>,
    args: RwLock<Box<Vec<Rc<ProofArgs>>>>,
    available_threads: Mutex<u32>,
}

type ObjName<'a> = &'a str;
type QueryPredicate<'a> = &'a str;
type QueryResult<'a> = HashMap<QueryPredicate<'a>,
                               HashMap<ObjName<'a>, Option<(bool, Option<Date>)>>>;

#[derive(Debug)]
pub struct InfResults<'a> {
    grounded_queries: RwLock<QueryResult<'a>>,
    membership: RwLock<HashMap<Rc<lang::Var>, HashMap<ObjName<'a>, Vec<Rc<GroundedClsMemb>>>>>,
}

impl<'a> InfResults<'a> {
    fn new() -> InfResults<'a> {
        InfResults {
            grounded_queries: RwLock::new(HashMap::new()),
            membership: RwLock::new(HashMap::new()),
        }
    }

    fn add_memberships(&self,
                       var: Rc<lang::Var>,
                       name: &'a str,
                       member_of: Vec<Rc<GroundedClsMemb>>) {
        let mut lock = self.membership.write().unwrap();
        lock.entry(var)
            .or_insert(HashMap::new())
            .insert(name, member_of);
    }

    fn add_grounded(&self, obj: &'a str, pred: &'a str, res: Option<(bool, Option<Date>)>) {
        let mut lock = self.grounded_queries.write().unwrap();
        lock.entry(pred).or_insert(HashMap::new()).insert(obj, res);
    }

    pub fn get_results_single(self) -> Option<bool> {
        let results = self.grounded_queries.into_inner().unwrap();
        if results.len() == 0 {
            return None;
        }
        for r0 in results.values() {
            for r1 in r0.values() {
                if let &Some((false, _)) = r1 {
                    return Some(false);
                }
                if let &None = r1 {
                    return None;
                }
            }
        }
        Some(true)
    }

    pub fn get_results_multiple(self) -> QueryResult<'a> {
        self.grounded_queries.into_inner().unwrap()
    }

    pub fn get_memberships(&'a self) -> HashMap<ObjName, Vec<&'a GroundedClsMemb>> {
        let lock = self.membership.read().unwrap();
        let mut res = HashMap::new();
        for preds in lock.values() {
            for members in preds.values() {
                for gr in members {
                    let gr = unsafe { &*(&**gr as *const GroundedClsMemb) as &'a GroundedClsMemb };
                    let name = unsafe { &*(&*gr.get_name() as *const String) as &'a str };
                    res.entry(name).or_insert(vec![]).push(gr);
                }
            }
        }
        res
    }
}

#[derive(Debug)]
pub enum Answer<'a> {
    Results(InfResults<'a>),
    QueryErr,
    ParseErr(ParseErrF),
}

impl<'a> Answer<'a> {
    pub fn get_results_single(self) -> Option<bool> {
        match self {
            Answer::Results(result) => result.get_results_single(),
            _ => panic!("simag: tried to unwrap a result from an error"),
        }
    }

    pub fn get_results_multiple(self) -> QueryResult<'a> {
        println!("RESULT:\n{:?}", self);
        match self {
            Answer::Results(result) => result.get_results_multiple(),
            _ => panic!("simag: tried to unwrap a result from an error"),
        }
    }

    pub fn get_memberships(&'a self) -> HashMap<ObjName, Vec<&'a GroundedClsMemb>> {
        match *self {
            Answer::Results(ref result) => result.get_memberships(),
            _ => panic!("simag: tried to unwrap a result from an error"),
        }
    }
}

impl<'a> Inference<'a> {
    fn new(agent: &'a Representation,
           query_input: QueryInput,
           ignore_current: bool)
           -> Result<Box<Inference<'a>>, ()> {
        let query = QueryProcessed::new().get_query(query_input)?;
        println!("QUERY:\n{:?}\n", query);
        Ok(Box::new(Inference {
            query: query,
            kb: agent,
            ignore_current: ignore_current,
            nodes: RwLock::new(HashMap::new()),
            queue: RwLock::new(HashMap::new()),
            results: InfResults::new(),
            repeat: Mutex::new(vec![]),
            args: RwLock::new(Box::new(vec![])),
            available_threads: Mutex::new(4_u32),
        }))
    }

    fn get_results(self) -> Answer<'a> {
        Answer::Results(self.results)
    }

    /// Inference function from first-order logic sentences.
    ///
    /// Gets a query from an 'ask' call, encapsulates the query subtitutions,
    /// processes it (including caching of partial results or tracking
    /// var substitution) and returns the answer to the query. If new
    /// knowledge is produced then it's passed to an other procedure for
    /// addition to the KB.
    fn infer_facts(&'a mut self) {
        fn query_cls<'a>(inf: &'a Inference<'a>, query: Rc<String>, actv_query: ActiveQuery<'a>) {
            let mut pass = Box::new(InfTrial::new(inf, actv_query));
            pass.get_rules(vec![query.clone()]);
            {
                let mut lock = inf.nodes.write().unwrap();
                for nodes in lock.values_mut() {
                    nodes.sort_by(|a, b| a.proof.created.cmp(&b.proof.created));
                }
            }
            // run the query, if there is no result and there is an update,
            // then loop again, else stop
            loop {
                pass.unify(query.clone(), VecDeque::new(), HashSet::new());
                {
                    let lock0 = pass.updated.lock().unwrap();
                    let lock1 = pass.feedback.lock().unwrap();
                    if !lock0.contains(&true) || !*lock1 {
                        break;
                    }
                }
                pass.updated = Mutex::new(vec![]);
            }
        }

        let inf_ptr = &*self as *const Inference as usize;
        let mut pool = Pool::new(*self.available_threads.lock().unwrap());

        pool.scoped(|scope| {
            for (obj, preds) in self.query.cls_queries_grounded.iter() {
                let obj: &str = &**obj;
                for pred in preds {
                    let pred_r = *pred as *const GroundedClsMemb as usize;
                    let query = unsafe { &*(&*pred.get_parent() as *const String) as &'static str };
                    scope.execute(move || {
                        let inf: &Inference;
                        let pred: &GroundedClsMemb;
                        unsafe {
                            inf = &*(inf_ptr as *const Inference);
                            pred = &*(pred_r as *const GroundedClsMemb);
                        }
                        let mut result = None;
                        if !inf.ignore_current {
                            result = inf.kb.class_membership(pred);
                        }
                        if result.is_some() {
                            inf.results.add_grounded(obj, query, Some((result.unwrap(), None)));
                        } else {
                            inf.results.add_grounded(obj, query, None);
                            // if no result was found from the kb directly
                            // make an inference from a grounded fact
                            let actv_query = ActiveQuery::Class(obj, &*query, pred);
                            query_cls(inf, pred.get_parent(), actv_query);
                        }
                    });
                }
            }
        });

        pool.scoped(|scope| {
            for pred in self.query.func_queries_grounded.iter() {
                let pred_r = &**pred as *const GroundedFunc as usize;
                scope.execute(move || {
                    let inf: &Inference;
                    let pred: &GroundedFunc;
                    let query: &str;
                    unsafe {
                        inf = &*(inf_ptr as *const Inference);
                        pred = &*(pred_r as *const GroundedFunc);
                        query = &*(&*pred.name as *const String) as &'static str;
                    }
                    let mut result = None;
                    for arg in pred.args.iter() {
                        let obj = unsafe { &*(&*arg.get_name() as *const String) as &'static str };
                        if !inf.ignore_current {
                            result = inf.kb.has_relationship(pred, arg.get_name());
                        }
                        if result.is_some() {
                            inf.results.add_grounded(&*obj, query, Some((result.unwrap(), None)));
                        } else {
                            inf.results.add_grounded(&*obj, query, None);
                            let actv_query = ActiveQuery::Func(&*obj, &*query, pred);
                            query_cls(inf, pred.get_name(), actv_query);
                        }
                    }
                });
            }
        });

        // cls_memb_query: HashMap<Rc<lang::Var>, Vec<&'a lang::FreeClsOwner>>
        for (var, objs) in self.query.cls_memb_query.iter() {
            for obj in objs {
                let name = unsafe { &*(&*obj.term as *const String) as &'a str };
                let member_of = self.kb.get_class_membership(obj);
                self.results.add_memberships(var.clone(), name, member_of);
            }
        }

        // func_memb_query: HashMap<Rc<lang::Var>, Vec<&'a lang::FreeClsOwner>>,
        for (var, objs) in self.query.func_memb_query.iter() {

        }
    }
}

struct InfTrial<'a> {
    kb: &'a Representation,
    actv: ActiveQuery<'a>,
    updated: Mutex<Vec<bool>>,
    feedback: Mutex<bool>,
    valid: Mutex<Option<(*const ProofNode, Rc<ProofArgs>)>>,
    nodes: &'a RwLock<HashMap<Rc<String>, Vec<Box<ProofNode>>>>,
    queue: &'a RwLock<HashMap<*const ProofNode, HashSet<PArgVal>>>,
    results: &'a InfResults<'a>,
    repeat: &'a Mutex<Vec<(Rc<LogSentence>, PArgVal)>>,
    args: &'a RwLock<Box<Vec<Rc<ProofArgs>>>>,
    available_threads: Mutex<u32>,
}

enum ActiveQuery<'a> {
    // `(obj_name, pred_name, fn/cls decl)`
    Class(&'a str, &'a str, &'a GroundedClsMemb),
    Func(&'a str, &'a str, &'a GroundedFunc),
}

impl<'a> ActiveQuery<'a> {
    #[inline]
    fn get_func(&self) -> &GroundedFunc {
        match self {
            &ActiveQuery::Func(_, _, ref gf) => gf,
            _ => panic!(),
        }
    }

    #[inline]
    fn get_cls(&self) -> &GroundedClsMemb {
        match self {
            &ActiveQuery::Class(_, _, ref gt) => gt,
            _ => panic!(),
        }
    }
}

type ProofArgs = Vec<(Rc<lang::Var>, Rc<VarAssignment>)>;

#[derive(Debug)]
pub struct ProofResult {
    pub result: Option<bool>,
    args: Rc<ProofArgs>,
    node: *const ProofNode,
    pub grounded: Vec<(lang::Grounded, Date)>,
}

impl ProofResult {
    fn new(args: Rc<ProofArgs>, node: &ProofNode) -> ProofResult {
        ProofResult {
            result: None,
            args: args,
            node: node as *const ProofNode,
            grounded: vec![],
        }
    }
}

impl<'a> InfTrial<'a> {
    fn new(inf: &'a Inference<'a>, actv_query: ActiveQuery<'a>) -> InfTrial<'a> {
        InfTrial {
            kb: inf.kb,
            actv: actv_query,
            updated: Mutex::new(vec![]),
            feedback: Mutex::new(true),
            valid: Mutex::new(None),
            nodes: &inf.nodes,
            queue: &inf.queue,
            results: &inf.results,
            repeat: &inf.repeat,
            args: &inf.args,
            available_threads: Mutex::new(4_u32),
        }
    }

    fn unify(&self,
             mut parent: Rc<String>,
             mut chk: VecDeque<Rc<String>>,
             mut done: HashSet<Rc<String>>) {

        fn scoped_exec(inf_ptr: usize, node_r: usize, args_r: usize) {
            let inf: &InfTrial;
            let node: &ProofNode;
            let args: Rc<ProofArgs>;
            unsafe {
                inf = &*(inf_ptr as *const InfTrial);
                node = &*(node_r as *const ProofNode);
                args = Rc::new(*Box::from_raw(args_r as *mut ProofArgs));
            }
            let arg_hash = arg_hash_val(&args);
            let node_r = node as *const ProofNode;
            let args_done = {
                if let Some(queued) = inf.queue.read().unwrap().get(&(node_r)) {
                    queued.contains(&arg_hash)
                } else {
                    false
                }
            };
            if !args_done {
                let mut n_args: HashMap<Rc<lang::Var>, &VarAssignment> =
                    HashMap::with_capacity(args.len());
                for &(ref k, ref v) in args.iter() {
                    n_args.insert(k.clone(), &*v);
                }
                let mut context = ProofResult::new(args.clone(), &node);
                node.proof.solve(inf.kb, Some(n_args), &mut context);
                if context.result.is_some() {
                    {
                        let mut lock0 = inf.updated.lock().unwrap();
                        lock0.push(true);
                        let mut lock1 = inf.queue.write().unwrap();
                        lock1.entry(node_r)
                            .or_insert(HashSet::new())
                            .insert(arg_hash);
                        let mut lock3 = inf.args.write().unwrap();
                        lock3.push(args.clone());
                    }
                    inf.add_result(context);
                }
            }
        };

        let inf_ptr = &*self as *const InfTrial as usize;
        loop {
            let mut pool = Pool::new(*self.available_threads.lock().unwrap());
            {
                *self.valid.lock().unwrap() = None;
            }
            // for each node in the subtitution tree unifify variables
            // and try every possible substitution until (if) a solution is found
            // the proofs are tried in order of addition to the KB
            if let Some(nodes) = self.nodes.read().unwrap().get(&parent) {
                // the node for each rule is stored in an efficient sorted list
                // by rule creation datetime, from newest to oldest
                // as the newest rules take precedence
                for node in nodes.iter() {
                    // recursively try unifying all possible argument with the
                    // operating logic sentence:
                    // get all the entities/classes from the kb that meet the proof requeriments
                    let assignments = self.meet_sent_req(node.proof.var_req.as_ref().unwrap());
                    if assignments.is_none() {
                        for e in node.antecedents.clone() {
                            if !done.contains(&e) && !chk.contains(&e) {
                                chk.push_back(e);
                            }
                        }
                        continue;
                    }
                    // lazily iterate over all possible combinations of the substitutions
                    let mapped = ArgsProduct::product(assignments.unwrap());
                    if mapped.is_some() {
                        let mapped = mapped.unwrap();
                        pool.scoped(|scope| {
                            let node_r = &**node as *const ProofNode as usize;
                            for args in mapped {
                                {
                                    let lock = self.valid.lock().unwrap();
                                    if lock.is_some() {
                                        break;
                                    }
                                }
                                let args_r = Box::into_raw(Box::new(args)) as usize;
                                scope.execute(move || scoped_exec(inf_ptr, node_r, args_r));
                            }
                        });
                    }
                    let lock = self.feedback.lock().unwrap();
                    if *lock {
                        for e in node.antecedents.clone() {
                            if !done.contains(&e) && !chk.contains(&e) {
                                chk.push_back(e);
                            }
                        }
                    }
                    /*
                    {
                        let ref lock0 = *self.valid.lock().unwrap();
                        let inner = unsafe {
                            &*(&*lock0.borrow() as *const Option<(Rc<ProofNode>, ProofArgs)>)
                        };
                        match inner {
                            &Some((ref node, ref assigned)) => {
                                // the result may be replaced in the KB by other proof which is less
                                // current, to ensure that the valid result stays in the KB after
                                // all proofs are done, repeat the valid one with proper arguments
                                let mut lock1 = self.repeat.lock().unwrap();
                                lock1.push((node.proof.clone(), assigned.clone()))
                            }
                            &None => {
                                for e in node.antecedents.clone() {
                                    if !done.contains(&e) && !chk.contains(&e) {
                                        chk.push_back(e);
                                    }
                                }
                            }
                        }
                    }
                    */
                }
            }
            let lock = self.feedback.lock().unwrap();
            if !*lock {
                return;
            } else if chk.len() > 0 {
                done.insert(parent);
                self.get_rules(Vec::from_iter(chk.iter().map(|x| x.clone())));
                let p = chk.pop_front().unwrap();
                parent = p;
            } else {
                return;
            }
        }
    }

    fn add_result(&self, mut context: ProofResult) {
        // add category/function to the object dictionary
        // and to results dict if is the result for the query
        let (query_obj, query_pred, is_func) = match self.actv {
            ActiveQuery::Class(ref obj, ref query_pred, _) => (obj, query_pred, false),
            ActiveQuery::Func(ref obj, ref query_pred, _) => (obj, query_pred, true),
        };
        if let Some(false) = context.result {
            self.results.add_grounded(query_obj, query_pred, Some((false, None)));
            return;
        }
        for subst in context.grounded.drain(..) {
            match subst {
                (lang::Grounded::Function(gf), date) => {
                    let gf: &GroundedFunc = &*gf;
                    if is_func {
                        let query_func = self.actv.get_func();
                        if query_func.comparable(&gf) {
                            let val;
                            if query_func == gf {
                                val = true;
                            } else {
                                val = false;
                            }
                            let mut d = self.results.grounded_queries.write().unwrap();
                            let mut d = d.entry(query_pred).or_insert(HashMap::new());
                            if d.contains_key(query_obj) {
                                let cond_ok;
                                if let Some(&Some((_, Some(ref cdate)))) = d.get(query_obj) {
                                    if &date >= cdate {
                                        cond_ok = true;
                                    } else {
                                        cond_ok = false;
                                    }
                                } else {
                                    cond_ok = true;
                                }
                                if cond_ok {
                                    d.insert(query_obj, Some((val, Some(date))));
                                    let mut lock = self.valid.lock().unwrap();
                                    *lock = Some((context.node, context.args.clone()));
                                }
                            } else {
                                d.insert(query_obj, Some((val, Some(date))));
                                let mut lock = self.valid.lock().unwrap();
                                *lock = Some((context.node, context.args.clone()));
                            }
                            let mut lock = self.feedback.lock().unwrap();
                            *lock = false;
                        }
                    }
                }
                (lang::Grounded::Terminal(gt), date) => {
                    if !is_func {
                        let query_cls = self.actv.get_cls();
                        if query_cls.comparable(&gt) {
                            let gt: &GroundedClsMemb = &*gt;
                            let val;
                            if query_cls == gt {
                                val = true;
                            } else {
                                val = false;
                            }
                            let mut d = self.results.grounded_queries.write().unwrap();
                            let mut d = d.entry(query_pred).or_insert(HashMap::new());
                            if d.contains_key(query_obj) {
                                let cond_ok;
                                if let Some(&Some((_, Some(ref cdate)))) = d.get(query_obj) {
                                    if &date >= cdate {
                                        cond_ok = true;
                                    } else {
                                        cond_ok = false;
                                    }
                                } else {
                                    cond_ok = true;
                                }
                                if cond_ok {
                                    let mut lock = self.valid.lock().unwrap();
                                    d.insert(query_obj, Some((val, Some(date))));
                                    *lock = Some((context.node.clone(), context.args.clone()));
                                }
                            } else {
                                d.insert(query_obj, Some((val, Some(date))));
                                let mut lock = self.valid.lock().unwrap();
                                *lock = Some((context.node.clone(), context.args.clone()));
                            }
                            let mut lock = self.feedback.lock().unwrap();
                            *lock = false;
                        }
                    }
                }
            }
        }
    }

    fn meet_sent_req(&self,
                     req: &HashMap<Rc<lang::Var>, Vec<Rc<lang::Assert>>>)
                     -> Option<HashMap<Rc<lang::Var>, Vec<Rc<VarAssignment>>>> {
        let mut results: HashMap<Rc<lang::Var>, Vec<Rc<VarAssignment>>> = HashMap::new();
        for (var, asserts) in req.iter() {
            let mut class_list = Vec::new();
            let mut funcs_list = Vec::new();
            for a in asserts {
                match **a {
                    lang::Assert::FuncDecl(ref f) => {
                        funcs_list.push(f);
                    }
                    lang::Assert::ClassDecl(ref c) => {
                        class_list.push(c.get_name());
                    }
                }
            }
            // meet_cls_req: HashMap<Rc<String>, Vec<Rc<GroundedClsMemb>>>
            let meet_cls_req = self.kb.by_class(&class_list);
            // meet_func_req: HashMap<Rc<String>, HashMap<Rc<String>, Vec<Rc<GroundedFunc>>>>
            let mut meet_func_req = self.kb.by_relationship(&funcs_list);
            let mut i0: HashMap<Rc<String>, usize> = HashMap::new();
            for (_, v) in meet_cls_req.iter() {
                for name in v.iter().map(|x| x.get_name()) {
                    let cnt: &mut usize = i0.entry(name).or_insert(0);
                    *cnt += 1;
                }
            }
            let mut i1: HashMap<Rc<String>, usize> = HashMap::new();
            for (_, v) in meet_func_req.iter() {
                for name in v.iter().map(|(name, _)| name) {
                    let cnt: &mut usize = i1.entry(name.clone()).or_insert(0);
                    *cnt += 1;
                }
            }
            let i2: Vec<_>;
            let cls_filter = i0.iter()
                .filter(|&(_, cnt)| *cnt == class_list.len())
                .map(|(k, _)| k.clone());
            let func_filter = i1.iter()
                .filter(|&(_, cnt)| *cnt == funcs_list.len())
                .map(|(k, _)| k.clone());
            if meet_func_req.len() > 0 && meet_cls_req.len() > 0 {
                let c1: HashSet<Rc<String>> = cls_filter.collect();
                i2 = func_filter.filter_map(|n0| c1.get(&n0)).map(|x| x.clone()).collect();
            } else if meet_func_req.len() > 0 {
                i2 = func_filter.collect();
            } else {
                i2 = cls_filter.collect();
            }
            for name in i2 {
                let mut gr_memb: HashMap<Rc<String>, Rc<GroundedClsMemb>> = HashMap::new();
                let mut gr_relations: HashMap<Rc<String>, Vec<Rc<GroundedFunc>>> = HashMap::new();
                for ls in meet_cls_req.values() {
                    for e in ls {
                        if e.get_name() == name {
                            gr_memb.insert(e.get_parent(), e.clone());
                        }
                    }
                }
                for (k, map) in meet_func_req.iter_mut() {
                    let ls = map.remove(&name).unwrap();
                    gr_relations.insert(k.clone(), ls);
                }
                if results.contains_key(var) {
                    let v = results.get_mut(var).unwrap();
                    v.push(Rc::new(VarAssignment {
                        name: name.clone(),
                        classes: gr_memb,
                        funcs: gr_relations,
                    }))
                } else {
                    results.insert(var.clone(),
                                   vec![Rc::new(VarAssignment {
                                            name: name.clone(),
                                            classes: gr_memb,
                                            funcs: gr_relations,
                                        })]);
                }
            }
            if !results.contains_key(var) {
                return None;
            }
        }
        Some(results)
    }

    fn get_rules(&self, cls_ls: Vec<Rc<String>>) {
        let mut rules = HashSet::new();
        for vrules in self.nodes.read().unwrap().values() {
            for r in vrules {
                rules.insert(r.proof.clone());
            }
        }
        for cls in cls_ls {
            if let Some(stored) = self.kb.classes.read().unwrap().get(&cls) {
                let lock = stored.beliefs.read().unwrap();
                let a: HashSet<Rc<LogSentence>> =
                    HashSet::from_iter(lock.get(&cls).unwrap().iter().map(|x| x.clone()));
                for sent in a.difference(&rules) {
                    let mut antecedents = vec![];
                    for p in sent.get_lhs_predicates() {
                        antecedents.push(p.get_name())
                    }
                    let node = Box::new(ProofNode::new(sent.clone(), antecedents.clone()));
                    for pred in sent.get_rhs_predicates() {
                        let name = pred.get_name();
                        let mut lock = self.nodes.write().unwrap();
                        let mut ls = lock.entry(name).or_insert(vec![]);
                        if let None = ls.iter()
                            .map(|x| x.proof.get_id())
                            .find(|x| *x == sent.get_id()) {
                            ls.push(node.clone());
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct ArgsProduct {
    indexes: HashMap<Rc<lang::Var>, (usize, bool)>,
    input: ArgsProductInput,
    curr: Rc<lang::Var>,
}

type ArgsProductInput = HashMap<Rc<lang::Var>, Vec<Rc<VarAssignment>>>;

impl ArgsProduct {
    fn product(input: ArgsProductInput) -> Option<ArgsProduct> {
        let mut indexes = HashMap::new();
        let mut curr = None;
        let mut first = true;
        for (k, _) in input.iter() {
            if first {
                curr = Some(k.clone());
                first = false;
            }
            indexes.insert(k.clone(), (0_usize, false));
        }
        if curr.is_some() {
            Some(ArgsProduct {
                indexes: indexes,
                input: input,
                curr: curr.unwrap(),
            })
        } else {
            None
        }
    }
}

impl ::std::iter::Iterator for ArgsProduct {
    type Item = ProofArgs;

    fn next(&mut self) -> Option<ProofArgs> {
        let mut row_0 = vec![];
        for (k1, v1) in self.input.iter() {
            let idx_1 = self.indexes.get(k1).unwrap();
            let e = (k1.clone(), v1[idx_1.0].clone());
            row_0.push(e);
        }
        if self.completed_iter() {
            None
        } else {
            Some(row_0)
        }
    }
}

impl ArgsProduct {
    fn completed_iter(&mut self) -> bool {
        let mut max = 0;
        for v in self.indexes.values() {
            if v.1 {
                max += 1;
            }
        }
        if max == self.indexes.len() {
            true
        } else {
            let p;
            {
                let l = self.input.get(&self.curr).unwrap().len();
                let curr = self.indexes.get(&self.curr).unwrap();
                if curr.0 < l - 1 {
                    p = true;
                } else {
                    p = false;
                }
            }
            if p {
                let mut i = 0;
                for (j, (k, v)) in self.indexes.iter_mut().enumerate() {
                    i = j;
                    let l = self.input.get(k).unwrap().len();
                    if (*k != self.curr) && (v.0 < l - 1) {
                        v.0 += 1;
                        break;
                    }
                }
                let lidx = self.indexes.len() - 1;
                let mut curr = self.indexes.get_mut(&self.curr).unwrap();
                let l = self.input.get(&self.curr).unwrap().len();
                if (i == lidx) && (curr.0 < l) {
                    curr.0 += 1;
                }
            } else {
                for k in self.indexes.keys() {
                    if *k != self.curr {
                        self.curr = k.clone();
                        break;
                    }
                }
                let mut curr = self.indexes.get_mut(&self.curr).unwrap();
                curr.1 = true;
                curr.0 = 0;
            }
            false
        }
    }
}

fn arg_hash_val(input: &ProofArgs) -> Vec<usize> {
    // ProofArgs: Vec<(Rc<lang::Var>, Rc<VarAssignment>)>;
    let mut v = vec![];
    for &(ref var, ref assigned) in input {
        v.push(&**var as *const lang::Var as usize);
        v.push(&*assigned.name as *const String as usize);
    }
    v
}

#[derive(Debug, Clone)]
struct ProofNode {
    proof: Rc<LogSentence>,
    antecedents: Vec<Rc<String>>,
}

impl ProofNode {
    fn new(proof: Rc<LogSentence>, antecedents: Vec<Rc<String>>) -> ProofNode {
        ProofNode {
            proof: proof.clone(),
            antecedents: antecedents,
        }
    }
}

impl<'a> ::std::cmp::PartialEq for ProofNode {
    fn eq(&self, other: &ProofNode) -> bool {
        self.proof.get_id() == other.proof.get_id()
    }
}

impl<'a> ::std::cmp::Eq for ProofNode {}

impl<'a> Hash for ProofNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.proof.get_id().hash(state);
    }
}

#[derive(Debug, Clone)]
pub struct VarAssignment {
    pub name: Rc<String>,
    classes: HashMap<Rc<String>, Rc<GroundedClsMemb>>,
    funcs: HashMap<Rc<String>, Vec<Rc<GroundedFunc>>>,
}

impl VarAssignment {
    #[inline]
    pub fn get_class(&self, name: Rc<String>) -> Option<&Rc<GroundedClsMemb>> {
        self.classes.get(&name)
    }

    #[inline]
    pub fn get_relationship(&self, func: &GroundedFunc) -> Option<&Rc<GroundedFunc>> {
        if let Some(funcs) = self.funcs.get(&func.get_name()) {
            for owned_f in funcs {
                if owned_f.comparable(func) {
                    return Some(owned_f);
                }
            }
        }
        None
    }
}

impl ::std::cmp::PartialEq for VarAssignment {
    fn eq(&self, other: &VarAssignment) -> bool {
        *self.name == *other.name
    }
}

impl ::std::cmp::Eq for VarAssignment {}

impl Hash for VarAssignment {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (*self.name).hash(state);
    }
}

#[derive(Debug)]
enum QueryInput {
    AskRelationalFunc(lang::FuncDecl),
    AskClassMember(lang::ClassDecl),
    ManyQueries(VecDeque<ParseTree>),
}

#[derive(Debug)]
struct QueryProcessed<'a> {
    cls_queries_free: HashMap<Rc<lang::Var>, Vec<&'a lang::FreeClsMemb>>,
    cls_queries_grounded: HashMap<Rc<String>, Vec<&'a lang::GroundedClsMemb>>,
    cls_memb_query: HashMap<Rc<lang::Var>, Vec<&'a lang::FreeClsOwner>>,
    func_queries_free: HashMap<Rc<lang::Var>, Vec<Rc<lang::FuncDecl>>>,
    func_queries_grounded: Vec<Box<lang::GroundedFunc>>,
    func_memb_query: HashMap<Rc<lang::Var>, Vec<&'a lang::FreeClsOwner>>,
    vars: Vec<Rc<lang::Var>>,
    cls: Vec<Rc<lang::ClassDecl>>,
    func: Vec<Rc<lang::FuncDecl>>,
}

impl<'a> QueryProcessed<'a> {
    fn new() -> QueryProcessed<'a> {
        QueryProcessed {
            cls_queries_free: HashMap::new(),
            cls_queries_grounded: HashMap::new(),
            cls_memb_query: HashMap::new(),
            func_queries_free: HashMap::new(),
            func_queries_grounded: vec![],
            func_memb_query: HashMap::new(),
            vars: vec![],
            cls: vec![],
            func: vec![],
        }
    }

    fn get_query(mut self, prequery: QueryInput) -> Result<QueryProcessed<'a>, ()> {
        fn assert_memb(query: &mut QueryProcessed, cdecl: Rc<lang::ClassDecl>) -> Result<(), ()> {
            let cdecl = unsafe { &*(&cdecl as *const Rc<lang::ClassDecl>) as &Rc<lang::ClassDecl> };
            match cdecl.get_parent() {
                &lang::Terminal::GroundedTerm(_) => {
                    for a in cdecl.get_args() {
                        match a {
                            &lang::Predicate::FreeClsMemb(ref t) => {
                                query.push_to_clsquery_free(t.get_var(), t);
                            }
                            &lang::Predicate::GroundedClsMemb(ref t) => {
                                query.push_to_clsquery_grounded(t.get_name(), t);
                            }
                            _ => return Err(()), // not happening ever
                        }
                    }
                }
                &lang::Terminal::FreeTerm(_) => {
                    for a in cdecl.get_args() {
                        match a {
                            &lang::Predicate::FreeClsOwner(ref t) => {
                                query.ask_class_memb(t);
                            }
                            _ => return Err(()), // not happening ever
                        }
                    }
                }
                _ => return Err(()),
            }
            Ok(())
        }

        fn assert_rel(query: &mut QueryProcessed, fdecl: Rc<lang::FuncDecl>) -> Result<(), ()> {
            let fdecl = unsafe { &*(&fdecl as *const Rc<lang::FuncDecl>) as &Rc<lang::FuncDecl> };
            match fdecl.get_parent() {
                &lang::Terminal::GroundedTerm(_) => {
                    match fdecl.is_grounded() {
                        true => {
                            query.push_to_fnquery_grounded(fdecl.as_ref().clone().into_grounded());
                        }
                        false => {
                            for a in fdecl.get_args() {
                                if let &lang::Predicate::FreeClsMemb(ref t) = a {
                                    query.push_to_fnquery_free(t.get_var(), fdecl.clone());
                                }
                            }
                        }
                    }
                }
                &lang::Terminal::FreeTerm(_) => {
                    for a in fdecl.get_args() {
                        match a {
                            &lang::Predicate::FreeClsOwner(ref t) => {
                                query.ask_relationships(t);
                            }
                            _ => return Err(()), // not happening ever
                        }
                    }
                }
                _ => return Err(()), // keyword: incomprenhensible
            }
            Ok(())
        }

        match prequery {
            QueryInput::AskClassMember(cdecl) => {
                self.cls.push(Rc::new(cdecl));
                let cdecl = self.cls.last().unwrap().clone();
                assert_memb(&mut self, cdecl)?;
            }
            QueryInput::AskRelationalFunc(fdecl) => {
                self.func.push(Rc::new(fdecl));
                let fdecl = self.func.last().unwrap().clone();
                assert_rel(&mut self, fdecl)?;
            }
            QueryInput::ManyQueries(trees) => {
                for parsetree in trees {
                    match parsetree {
                        lang::ParseTree::Assertion(assertions) => {
                            for a in assertions {
                                if let Err(()) = match a {
                                    lang::Assert::ClassDecl(cdecl) => {
                                        self.cls.push(Rc::new(cdecl));
                                        let cdecl = self.cls.last().unwrap().clone();
                                        assert_memb(&mut self, cdecl)
                                    }
                                    lang::Assert::FuncDecl(fdecl) => {
                                        self.func.push(Rc::new(fdecl));
                                        let fdecl = self.func.last().unwrap().clone();
                                        assert_rel(&mut self, fdecl)
                                    }
                                } {
                                    return Err(());
                                }
                            }
                        }
                        lang::ParseTree::Expr(expr) => {
                            let (mut vars, preds) = expr.extract_all_predicates();
                            self.vars.append(&mut vars);
                            for a in preds {
                                if let Err(()) = match *a {
                                    lang::Assert::ClassDecl(ref cdecl) => {
                                        self.cls.push(Rc::new(cdecl.clone()));
                                        let cdecl = self.cls.last().unwrap().clone();
                                        assert_memb(&mut self, cdecl)
                                    }
                                    lang::Assert::FuncDecl(ref fdecl) => {
                                        self.func.push(Rc::new(fdecl.clone()));
                                        let fdecl = self.func.last().unwrap().clone();
                                        assert_rel(&mut self, fdecl)
                                    }
                                } {
                                    return Err(());
                                }
                            }
                        }
                        _ => return Err(()), // an iexpr: incomprehensible
                    }
                }
            }
        }
        Ok(self)
    }

    #[inline]
    fn push_to_clsquery_grounded(&mut self, term: Rc<String>, cls: &'a lang::GroundedClsMemb) {
        self.cls_queries_grounded.entry(term).or_insert(vec![]).push(cls);
    }

    #[inline]
    fn push_to_clsquery_free(&mut self, term: Rc<lang::Var>, cls: &'a lang::FreeClsMemb) {
        self.cls_queries_free.entry(term).or_insert(vec![]).push(cls);
    }

    #[inline]
    fn push_to_fnquery_grounded(&mut self, func: lang::GroundedFunc) {
        self.func_queries_grounded.push(Box::new(func))
    }

    #[inline]
    fn push_to_fnquery_free(&mut self, term: Rc<lang::Var>, func: Rc<lang::FuncDecl>) {
        self.func_queries_free.entry(term).or_insert(vec![]).push(func);
    }

    #[inline]
    fn ask_class_memb(&mut self, term: &'a lang::FreeClsOwner) {
        self.cls_memb_query.entry(term.parent.clone()).or_insert(vec![]).push(term);
    }

    #[inline]
    fn ask_relationships(&mut self, term: &'a lang::FreeClsOwner) {
        self.func_memb_query.entry(term.parent.clone()).or_insert(vec![]).push(term);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn _temp() {
        let test_08 = String::from("
            # retrieve all relations between objects
            (fn::loves[$Vicky,u=1;$Lucy])
            (fn::hates[$Vicky,u=1;cats])
            (fn::hates[$Vicky,u=0;dogs])
        ");
        let q08_01 = "((let x) (fn::x[$Vicky,u>0;$Lucy]))".to_string();
        let q08_02 = "((let x, y) (fn::x[$Vicky,u>0;y]))".to_string();
        let rep = Representation::new();
        rep.tell(test_08).unwrap();
        let mut results = HashSet::new();
        results.insert("loves");
        results.insert("hates");
        rep.ask(q08_01);
        rep.ask(q08_02);
    }

    #[test]
    fn ask_pred() {
        let test_01 = String::from("
            ( professor[$Lucy,u=1] )
        ");
        let q01_01 = "(professor[$Lucy,u=1] && person[$Lucy,u=1])".to_string();
        let q01_02 = "(professor[$Lucy,u=1])".to_string();
        let rep = Representation::new();
        rep.tell(test_01).unwrap();
        assert_eq!(rep.ask(q01_01).get_results_single(), None);
        assert_eq!(rep.ask(q01_02).get_results_single(), Some(true));

        let test_02 = String::from("
            ( professor[$Lucy,u=1] )
            ( dean[$John,u=1] )
            ( ( let x ) ( dean[x,u=1] |> professor[x,u=1] ) )
            ( ( let x ) ( professor[x,u=1] |> person[x,u=1] ) )
        ");
        let q02_01 = "(professor[$Lucy,u>0] && person[$Lucy,u<1])".to_string();
        let q02_02 = "(person[$John,u=1])".to_string();
        let rep = Representation::new();
        rep.tell(test_02).unwrap();
        assert_eq!(rep.ask(q02_01).get_results_single(), Some(false));
        assert_eq!(rep.ask(q02_02).get_results_single(), Some(true));

        let test_03 = String::from("
            ( fn::owns[$M1,u=1;$Nono] )
            ( missile[$M1,u=1] )
            ( american[$West,u=1] )
            ( fn::enemy[$Nono,u=1;$America] )
            (( let x, y, z )
             (( american[x,u=1] && weapon[y,u=1] && fn::sells[y,u=1;x;z] && hostile[z,u=1]  )
                 |> criminal[x,u=1] ))
            (( let x )
             (( fn::owns[x,u=1;$Nono] && missile[x,u=1] ) |> fn::sells[x,u=1;$West;$Nono] ))
            (( let x ) ( missile[x,u=1] |> weapon[x,u=1] ) )
            (( let x ) ( fn::enemy[x,u=1;$America] |> hostile[x,u=1] ) )
        ");
        let q03_01 = "(criminal[$West,u=1]) && hostile[$Nono,u=1] && weapon[$M1,u=1]".to_string();
        let rep = Representation::new();
        rep.tell(test_03).unwrap();
        assert_eq!(rep.ask(q03_01).get_results_single(), Some(true));

        let _test_04 = String::from("
            (( let x, y, t1:time, t2:time=\"*now\" )
             (( dog[x,u=1] && meat[y,u=1] && fn::eat(t1=time)[y,u=1;x] && fn::time_calc(t1<t2) )
              |> fat(time=t2)[x,u=1] ))
            ( dog[$Pancho,u=1] )
            ( meat[$M1,u=1] )
            ( fn::eat(time=\"2015.07.05.10.25\")[$M1,u=1;$Pancho] )
        ");
        let _q04_01 = "(fat(t='*now')[$Pancho,u=1])".to_string();

        let _test_05 = String::from("
            (( let x, y, t1:time, t2:time=\"2016.01.01\" )
             (( (dog[x,u=1] && meat[y,u=1] && fn::eat(t1=time)[y,u=1;x]) && fn::time_calc(t1<t2) )
              |> fat(time=t2)[x,u=1] ))
            ( dog[$Pancho,u=1] )
            ( meat[$M1,u=1] )
            ( fn::eat(time=\"2015.07.05.10.25\")[$M1,u=1;$Pancho] )
        ");
        let _q05_01 = "(fat(t='*now')[$Pancho,u=1])".to_string();

        let test_06 = String::from("
            # query for all 'professor'
            ( professor[$Lucy,u=1] )
        	( dean[$John,u=1] )
        	((let x) (dean[x,u=1] |> professor[x,u=1]))
        ");
        let _q06_01 = "((let x) (professor[x,u=1]))".to_string();
        // {'$Lucy': {'professor': True}, '$John': {'professor': True}}
        let rep = Representation::new();
        rep.tell(test_06).unwrap();

        let test_07 = String::from("
            # query for all classes '$Lucy' is member of
            (professor[$Lucy,u=1])
        	((let x) (professor[x,u=1] |> person[x,u=1]))
        	(ugly[$Lucy,u=0.2])
        ");
        let q07_01 = "((let x) (x[$Lucy,u>0.5]))".to_string();
        let rep = Representation::new();
        rep.tell(test_07).unwrap();
        let mut results = HashSet::new();
        results.insert("professor");
        results.insert("person");
        let answ = rep.ask(q07_01);
        for a in answ.get_memberships().get("$Lucy").unwrap() {
            assert!(results.contains(&**a.get_parent()));
            assert!(&**a.get_parent() != "ugly");
        }
    }

    #[test]
    fn ask_func() {
        let test_01 = String::from("
            ( professor[$Lucy,u=1] )
            ( dean[$John,u=1] )
            ( fn::criticize[$John,u=1;$Lucy] )
        ");
        let q01_01 = "(fn::criticize[$John,u=1;$Lucy])".to_string();
        let rep = Representation::new();
        rep.tell(test_01).unwrap();
        assert_eq!(rep.ask(q01_01).get_results_single(), Some(true));

        let test_02 = String::from("
            ( animal[cow,u=1] )
            ( female[cow,u=1] )
            ( (animal[cow,u=1] && female[cow,u=1]) |> fn::produce[milk,u=1;cow] )
        ");
        let q02_01 = "(fn::produce[milk,u=1;cow])".to_string();
        let rep = Representation::new();
        rep.tell(test_02).unwrap();
        assert_eq!(rep.ask(q02_01).get_results_single(), Some(true));

        let test_03 = String::from("
            ( professor[$Lucy,u=1] )
            ( dean[$John,u=1] )
            ( fn::criticize[$John,u=1;$Lucy] )
            ( (let x) ( dean[x,u=1] |> professor[x,u=1] ) )
            ( (let x) ( professor[x,u=1] |> person[x,u=1] ) )
            ( (let x, y)
              (( person[x,u=1] && person[y,u=1] && dean[y,u=1] && fn::criticize[y,u=1;x] )
                 |> fn::friend[x,u=0;y] ))
        ");
        let q03_01 = "(fn::friend[$Lucy,u=0;$John])".to_string();
        let rep = Representation::new();
        rep.tell(test_03).unwrap();
        assert_eq!(rep.ask(q03_01).get_results_single(), Some(true));

        let _test_05 = String::from("
            (( let x, y, t1: time=\"2015.01.01\", t2: time=\"2015.02.01\" )
             ( ( dog[x,u=1] && meat[y,u=1] && fat(time=t2)[x,u=1] && fn::time_calc(t1<t2) )
               |> fn::eat(time=t1)[y,u=1;x]
             )
            )
            ( dog[$Pancho,u=1] )
            ( meat[$M1,u=1] )
            ( fat[$Pancho,u=1] )
        ");
        let _q05_01 = "(fn::eat[$M1,u=1;$Pancho])".to_string();

        let _test_06 = String::from("
        	(( let x, y, t1: time=\"2015.01.01\", t2: time )
             ( ( dog[x,u=1] && meat[y,u=1] && fat(time=t2)[x,u=1] && fn::time_calc(t1<t2) )
               |> fn::eat(time=t1)[y,u=1;x]
             )
            )
            ( dog[$Pancho,u=1] )
            ( meat[$M1,u=1] )
            ( fat(time=\"2015.12.01\")[$Pancho,u=1] )
        ");
        let _q06_01 = "(fn::eat[$M1,u=1;$Pancho])".to_string();

        let _test_07 = String::from("
            # retrieve all objs which fit to a criteria
            (cow[$Lucy,u=1])
            (goat[$Vicky,u=1])
            ((let x) ((cow[x,u=1] || goat[x,u=1]) |> (female[x,u=1] && animal[x,u=1])))
            ((let x) ((female[x,u>0] && animal[x,u>0]) |> fn::produce[milk,u=1;x]))
        ");
        let _q07_01 = "((let x) (fn::produce[milk,u>0;x]))".to_string();

        let _test_08 = String::from("
            # retrieve all relations between objects
            (fn::loves[$Vicky,u=1;$Lucy])
        ");
        let _q08_01 = "((let x) (fn::x[$Vicky,u>0;$Lucy]))".to_string();
    }
}
