# -*- coding: utf-8 -*-
"""Main knowledge-base logic module, in this module exist the different 
classes that transform and store the data for the individual agents and 
serve as representations of the different objects and the relationships 
between them.

Main
----
`Representation`: Main class, stores all the representations and
relationships for a given agent in a concrete time.

`Individual`: Represents a singular entity, which is the unique
member of it's own set.

`Categories`: The sets in which the agent can classify objects.
Also stores the types of relations an object can have.

Support classes (and methods) and functions
-------------------------------------------
`LogSentence`: Stores a serie of logical atoms (be them predicates or
connectives), that form a well-formed logic formula. These are rulesets 
for reasoning, cataloging objects into sets/classes, and the relationships 
between these objects. 

LogSentences are owned by the funtion 'make_logic_sentence', and cannot be
called directly.

`Inference`: Encapsulates the whole inference process, from making
a temporal substitution representation where the inference is operated to
solving the query (including query parsing, data fetching and unification).
"""

# TODO: on ASK, add functionality so it so it can deal with queries that
# ask about relations of the same type with several objects.
# TODO: support skolemization in forms and existential variables

# ===================================================================#
#   Imports and globals
# ===================================================================#

import uuid
import itertools
from datetime import datetime
from threading import Lock
from collections import OrderedDict, deque
from sortedcontainers import SortedListWithKey

from simag.core import bms
from simag.core.parser import (
    logic_parser,
    make_fact,
    LogFunction,
    LogPredicate,
    FreeTerm,
    NotCompAssertError,
    NotCompFuncError,
)

# ===================================================================#
#   REPRESENTATION OBJECTS CLASSES AND SUBCLASSES
# ===================================================================#

class Representation(object):
    """This class is a container for internal agent's representations. 
    An agent can have any number of such representations at any moment, 
    all of which are contained in this object.
    
    The class includes methods to encode and decode the representations 
    to/from data streams or idioms.
    
    Attributes:
        individuals -> Unique members (entities) of their own set/class.
        | Entities are denoted with a $ symbol followed by a name.
        classes -> Sets of objects that share a common property.
    """
    
    def __init__(self):
        """IMPORTANT! 
        A representation shouldn't be initialized directly, instead it must
        be initialized and owned by an agent. Agents are initialized from 
        Rust where the main event loop is controlled from to manage 
        concurrent operations and communication.
        """
        self.bmsWrapper = bms.BmsWrapper(self)
        self._locked = {}
        self.individuals = {}
        self.classes = {}
        #rust.start_event_loop(self)

    def tell(self, string):
        """Parses a sentence (or several of them) into an usable formula 
        and stores it into the internal representation along with the 
        corresponding classes. In case the sentence is a predicate, 
        the objects get declared as members of their classes.
        
        Accepts first-order logic sentences sentences, both atomic 
        sentences ('Lucy is a professor') and complex sentences compossed 
        of different atoms and operators ('If someone is a professor,
        then it's a person'). Examples:
        
        >>> r.tell("professor[$Lucy,u=1]")
        will include the individual '$Lucy' in the professor category)
        >>> r.tell(":vars:x: (professor[x,u=1] |= person[x,u=1])")
        all the individuals which are professors will be added to the
        person category, and the formula will be stored in the professor
        class for future use.
        
        For more examples check the LogSentence class docs.
        """
        results = logic_parser(string)
        for a in results.assert_memb:
            self.up_memb(a)
            self.bmsWrapper.add(a)
        for a in results.assert_rel:
            self.up_rel(a)
            self.bmsWrapper.add(a)
        for a in results.assert_rules:
            self.save_rule(a)
        for a in results.assert_cogs:
            self.add_cog(a)
    
    def ask(self, sent, single=True, **kwargs):
        """Asks the KB if some fact is true and returns the answer to 
        the query.
        """
        inf_proc = Inference(self, sent, **kwargs)
        if single is True:
            for answ in inf_proc.results.values():
                for pred in answ.values():
                    if pred is False: return False
                    if pred is None: return None
            return True
        return inf_proc.results

    def up_memb(self, pred, return_val=False):
        # It's a membership declaration.        
        subject, categ = pred.term, pred.parent
        if subject not in self.individuals and '$' in subject:
            # An individual which is member of a class
            ind = Individual(subject)
            pred = ind.add_ctg(pred, get_obj=True)
            self.individuals[subject] = ind
        elif '$' in subject:
            # Add/replace an other class membership to an existing individual
            pred = self.individuals[subject].add_ctg(pred, get_obj=True)
        elif subject in self.classes:
            pred = self.classes[subject].add_ctg(pred, get_obj=True)
        else:
            # Is a new subclass of an other class
            cls = Category(subject)
            cls.type_ = 'class'
            pred = cls.add_ctg(pred, get_obj=True)
            self.classes[subject] = cls
        if categ not in self.classes:
            nc = Category(categ)
            nc.type_ = 'class'
            self.classes[categ] = nc
        if return_val: return pred

    def up_rel(self, func, return_val=False):
        # It's a function declaration.
        relation = func.func
        for subject in func.get_args():
            if '$' in subject:
                # It's a rel between an object and other obj/class.
                if subject not in self.individuals:
                    ind = Individual(subject)
                    rel = ind.add_rel(func, get_obj=True)
                    self.individuals[subject] = ind
                else:
                    ind = self.individuals[subject]
                    rel = ind.add_rel(func, get_obj=True)
                if relation not in self.classes:
                    rel = Relation(relation)
                    self.classes[relation] = rel
            else:
                # It's a rel between a class and other class/obj.
                if subject not in self.classes:
                    categ = Category(subject)
                    rel = categ.add_rel(func, get_obj=True)
                    self.classes[subject] = categ
                else:
                    rel = self.classes[subject].add_rel(func, get_obj=True)
                if relation not in self.classes:
                    rel = Relation(relation)
                    self.classes[relation] = rel
        if return_val: return rel

    def add_cog(self, sent):                
        def chk_args(p):
            if hasattr(sent, 'var_order') and sbj in sent.var_order:
                if p in self.classes:
                    self.classes[p].add_cog(sent)
                else:
                    if not issubclass(pred.__class__, LogFunction): 
                        nc = Category(sbj)
                    else: 
                        nc = Relation(sbj)
                    nc.add_cog(sent)
                    self.classes[p] = nc
            else:
                if '$' in sbj and sbj in self.individuals:
                    self.individuals[sbj].add_cog(p, sent)
                elif '$' in sbj:
                    ind = Individual(sbj)
                    ind.add_cog(p, sent)
                    self.individuals[sbj] = ind
                elif sbj in self.classes:
                    self.classes[sbj].add_cog(sent)
                else:
                    if not issubclass(pred.__class__, LogFunction): 
                        nc = Category(sbj)
                    else: 
                        nc = Relation(sbj)
                    nc.add_cog(sent)
                    self.classes[sbj] = nc        
        preds = []
        for p in sent:
            if p.cond == ':predicate:':
                preds.append(p.pred)
        for pred in preds:
            if issubclass(pred.__class__, LogFunction):
                if hasattr(pred, 'args'):                    
                    for arg in pred.args:
                        if isinstance(arg, tuple): sbj = arg[0]
                        else: sbj = arg
                        chk_args(pred.func)
            elif issubclass(pred.__class__, LogPredicate):
                sbj, p = pred.term, pred.parent
                chk_args(p)
        questions = sent.get_preds('r', return_obj=True)
        for query in questions:
            self.ask(query)
        
    def save_rule(self, proof):
        preds = proof.get_all_preds()
        n = []
        for p in preds:
            if issubclass(p.__class__, LogFunction): name = p.func
            else: name = p.parent
            n.append(name)
            if name in self.classes \
            and proof not in self.classes[name].cog:
                self.classes[name].add_cog(proof)
            else:
                if not issubclass(p.__class__, LogFunction): 
                    nc = Category(name)
                else: 
                    nc = Relation(name)
                nc.add_cog(proof)
                self.classes[name] = nc
        # Run the new formula with individuals/classes that match.
        n = set(n)
        obj_dic = self.objs_by_ctg(n, 'individuals')
        cls_dic = self.objs_by_ctg(n, 'classes')
        obj_dic.update(cls_dic)
        for obj in obj_dic.keys():
            proof(self, obj)

    def objs_by_ctg(self, ctgs, type_):
        if type_== 'individuals':
            collect = self.individuals.values()
        elif type_ == 'classes':
            collect = self.classes.values()
        else: 
            raise ValueError("'type_' parameter must have one of the \
            following values: individuals, classes")
        ctg_dic = {}
        for ind in collect:
            s = ind.check_ctg(ctgs)
            t = set(ind.get_rel())
            t = t.intersection(ctgs)
            t = t.union(s)
            if len(t) != 0: ctg_dic[ind.name] = t
        return ctg_dic
    
    def test_pred(self, p, kls=None):
        if not kls:
            if issubclass(p.__class__, LogPredicate):
                kls = 'pred'
            elif issubclass(p.__class__, LogFunction):
                kls = 'func'
        if kls == 'pred':
            subject = p.term
            if subject[0] == '$':
                try: return self.individuals[subject].test_ctg(p)
                except: return None
            else:
                try: return self.individuals[subject].test_ctg(p)
                except: return None
        elif kls == 'func':
            for subject in p.get_args():
                if subject[0] == '$':
                    try: return self.individuals[subject].test_rel(p)
                    except: return None
                else:
                    try: return self.classes[subject].test_rel(p)
                    except: return None
        else:
            raise TypeError("the first argument must be of " \
                + "LogPredicate or LogFunction type")
    
    def thread_manager(self, to_lock, unlock=False):
        """This method is called to manage locks on working objects.
        
        When a substitution is done all the relevant objects are locked until
        it finishes to prevent race conditions.
        
        After it finishes the locks are freed so an other subtitution can pick
        up the request.
        """
        if unlock is True:
            for p in to_lock:
                lock = self._locked[p]
                lock.release()
                del self._locked[p]
            return
        for p in to_lock:
            if p in self._locked:
                self._locked[p].acquire(timeout=5)
            else:
                lock = Lock()
                self._locked[p] = lock
                lock.acquire(timeout=5)

# TODO: individuals/classes should be linked in a tree structure
# for more effitient retrieval. 
class Individual(object):
    """An individual is the unique member of it's own class.
    Represents an object which can pertain to multiple classes or sets.
    It's an abstraction owned by an agent, the internal representation 
    of the object, not the object itself.
    
    An Individual inherits the properties of the classes it belongs to,
    and has some implicit attributes which are unique to itself.
    
    Membership to a class is denoted (following fuzzy sets) by a
    real number between 0 and 1. If the number is one, the object will
    will always belong to the set, if it's zero, it will never belong to
    the set.
    
    For example, an object can belong to the set 'cold' with a degree of
    0.9 (in natural language then it would be 'very cold') or 0.1
    (then it would be 'a bit cold', the subjective adjectives are defined
    in the category itself).
    
    Attributes:
        id -> Unique identifier for the object.
        name -> Name of the unique object.
        categ -> Categories to which the object belongs.
        | Includes the degree of membership (ie. ('cold', 0.9)).
        attr -> Implicit attributes of the object, unique to itself.
        cog (opt) -> These are the cognitions/relations attributed to the
        | object by the agent owning this representation.
        relations (opt) -> Functions between objects and/or classes.
    """
    def __init__(self, name):
        self.id = str(uuid.uuid4()).replace('-','')
        self.name = name
        self.categ = []
        self.relations = {}
        self.cog = OrderedDict()

    def set_attr(self, **kwargs):
        """Sets implicit attributes for the class, if an attribute exists
        it's replaced.
        
        Takes a dictionary as input.
        """
        if not hasattr(self, 'attr'):
            self.attr = {}
        for k, v in kwargs.items():
            self.attr[k] = v

    def infer(self):
        """Inferes attributes of the entity from it's classes."""
        pass
    
    def add_cog(self, p, sent):
        if p in self.cog and sent not in self.cog[p]:
            self.cog[p].append(sent)
        else:
            self.cog[p] = [sent]
        
    def add_ctg(self, fact, get_obj=False):
        if issubclass(fact.__class__, LogPredicate):
            ctg_rec = [f.parent for f in self.categ]
            try: idx = ctg_rec.index(fact.parent)
            except ValueError: 
                self.categ.append(fact)
                if get_obj: return fact
            else:
                self.categ[idx].update(fact)
                if get_obj: return self.categ[idx]
        else: raise TypeError('The object is not a LogPredicate subclass.')
    
    def check_ctg(self, n):
        """Returns a list that is the intersection of the input iterable
        and the categories of the object.
        """
        s = [c.parent for c in self.categ if c.parent in n]
        return s

    def get_ctg(self, ctg=None, obj=False):
        """Returns a dictionary of the categories of the object and
        their truth values.
        
        If a single category is provided in the 'ctg' keyword argument,
        then the value for that category is returned. If it doesn't
        exist, None is returned.
        
        If the obj keyword parameter is True then the object is returned.
        """
        if obj is True:
            if type(ctg) is str:
                for c in self.categ:
                    if c.parent == ctg: return c
            else:
                assert issubclass(ctg.__class__, LogPredicate), \
                "{0} is not subclass of LogPredicate".format(ctg.__class__)
                for c in self.categ:
                    if ctg == c: return c
            return None
        
        cat = {c.parent:c.value for c in self.categ}
        if ctg is None:
            return cat
        else:
            try: x = cat[ctg]
            except KeyError: return None
            else: return x
    
    def test_ctg(self, pred, obj=False):
        """Checks if it's child of a category and returns true if it's 
        equal to the comparison, false if it's not, and none if it
        doesn't exist.
        """
        for ctg in self.categ:
            if ctg.parent == pred.parent:
                categ = ctg
                break
        if 'categ' not in locals(): return None
        if pred == categ: 
            if obj: return categ
            return True
        else: return False
    
    def add_rel(self, func, get_obj=False):
        try:
            rels = self.relations[func.func]
        except KeyError:
            self.relations[func.func] = [func]
            if get_obj: return func
        else:
            for rel in rels:
                if rel.chk_args_eq(func) is True:
                    rel.update(func)
                    if get_obj: return rel
                    break
        
    def get_rel(self, func=None):
        """Returns a list of the relations the object is involved either
        as subject, object or indirect object.
        
        If a function is provided for comparison then the original function
        is returned.
        """
        if func:
            try:
                funcs = self.relations[func.func]
            except KeyError:
                return None      
            for f in funcs:
                if f.args_ID == func.args_ID: return f            
        rel = [k for k in self.relations]
        return rel
    
    def test_rel(self, func, obj=False, copy_date=False):
        """Checks if a relation exists; and returns true if it's equal
        to the comparison, false if it's not, and None if it doesn't exist.
        """
        try:
            funcs = self.relations[func.func]
        except KeyError:
            return None      
        for f in funcs:
            if f.args_ID == func.args_ID:
                if func == f:
                    if copy_date and hasattr(f, 'dates'):
                        func.dates = f.dates
                    if obj:
                        return f
                    return True
                else: return False
        return None
    
    def get_date(self, pred):
        """Take a predicate or a function and return the times at which
        those where true or false. If it doesn't exist returns 'None'.
        If it does not have a datetime object attached then the current
        datetime object is returned.
        """
        if issubclass(pred.__class__, LogFunction):
            func = pred
            try: funcs = self.relations[func.func]
            except KeyError: return None
            for f in funcs:
                if func.args_ID == f.args_ID:
                    if func == f:
                        if hasattr(f, 'dates'): return f.dates[-1]
                        else: return datetime.now()
                    else: return False
        elif issubclass(pred.__class__, LogPredicate):
            fact = pred
            ctg_rec = [f.parent for f in self.categ]
            try: idx = ctg_rec.index(fact.parent)
            except ValueError: return None   
            else:
                if fact == self.categ[idx]:
                    if hasattr(self.categ[idx], 'dates'): 
                        return self.categ[idx].dates[-1]
                    else: return datetime.now()
                else: return False
    
    def __str__(self):
        s = "<individual '" + self.name + "' w/ id: " + self.id + ">"
        return s

class Category(object):
    """A category is a set/class of entities that share some properties.    
    It can be a subset of others supersets, and viceversa.
    
    Membership is not binary, but fuzzy, being the extreme cases (0, 1)
    the classic binary membership. Likewise, membership to a class can be 
    temporal. For more info check 'Individual' class.
    
    All the attributes of a category are inherited by their members
    (to a degree).
    """
    def __init__(self, name, **kwargs):
        self.name = name
        self.cog = []
        if kwargs:
            for k, v in kwargs.items():
                if k == 'parents': setattr(self, 'parents', v)
                else: self[k] = v
    
    def infer(self):
        """Infers attributes of the class from it's members."""
        pass
    
    def add_cog(self, sent):
        if sent not in self.cog: self.cog.append(sent)
    
    def add_rel(self, func, get_obj=False):
        if not hasattr(self, 'relations'):
            self.relations = dict()
            self.relations[func.func] = [func]
            if get_obj: return func
        else:
            try: rels = self.relations[func.func]
            except KeyError: 
                self.relations[func.func] = [func]
                if get_obj: return func
            else:
                for rel in rels:
                    if rel.chk_args_eq(func) is True:
                        rel.update(func)
                        if get_obj: return rel
                        break
    
    def get_rel(self, func=None):
        """Returns a list of the relations the object is involved either
        as subject, object or indirect object.
        
        If a function is provided for comparison then the original function
        is returned.
        """
        if func:
            try:
                if hasattr(self, 'relations'):
                    funcs = self.relations[func.func]
                else: return None
            except KeyError:
                return None      
            for f in funcs:
                if f.args_ID == func.args_ID: return f
            return None        
        if hasattr(self, 'relations'): rel = [k for k in self.relations]
        else: rel = []
        return rel
    
    def test_rel(self, func, obj=False, copy_date=False):
        """Checks if a relation exists; and returns true if it's 
        equal to the comparison, false if it's not, and None if it
        doesn't exist.
        """
        try:
            funcs = self.relations[func.func]
        except (KeyError, AttributeError):
            return None      
        for f in funcs:
            if f.args_ID == func.args_ID:
                if func == f:
                    if copy_date and hasattr(f, 'dates'):
                        func.dates = f.dates
                    if obj:
                        return obj
                    return True
                else: return False
        return None
    
    def add_ctg(self, fact, get_obj=False):
        if not hasattr(self,'parents'): self.parents = [fact]
        else:
            ctg_rec = [f.parent for f in self.parents]
            try: idx = ctg_rec.index(fact.parent)
            except ValueError: 
                self.parents.append(fact)
                if get_obj: return fact       
            else:
                self.parents[idx].update(fact)
                if get_obj: return self.parents[idx]
    
    def get_ctg(self, ctg=None, obj=False):
        """Returns a dictionary of the categories of the object and
        their truth values.
        
        If a single category is provided in the 'ctg' keyword argument,
        then the value for that category is returned. If it doesn't
        exist, None is returned.
        
        If the obj keyword parameter is True then the object is returned.
        """
        if obj is True:
            assert issubclass(ctg.__class__, LogPredicate), \
            "'ctg' is not subclass of LogPredicate"
            for c in self.parents:
                if ctg == c: return c
            return None
        
        cat = {c.parent:c.value for c in self.parents}
        if ctg is None:
            return cat
        else:
            try: x = cat[ctg]
            except KeyError: return None
            else: return x
    
    def test_ctg(self, pred, obj=False):
        """Checks if it's child of a category and returns true if it's 
        equal to the comparison, false if it's not, and none if it
        doesn't exist.
        """        
        if not hasattr(self,'parents'): return None
        for ctg in self.parents:
            if ctg.parent == pred.parent:
                categ = ctg
                break
        if 'categ' not in locals(): return None
        if pred == categ:
            if obj: return categ 
            return True
        else: return False
        
        
    def check_ctg(self, n):
        """Returns a list that is the intersection of the input iterable
        and the parents of the object.
        """
        if not hasattr(self,'parents'): return list()
        return [c.parent for c in self.parents if c.parent in n]
    
class Relation(Category):
    
    @property
    def add_rel(self, func):
        raise AttributeError("'Relation' object has no attribute 'add_rel'.")

class Group(Category):
    """A special instance of a category. It defines a 'group' of
    elements that pertain to a class.
    """

class Part(Category):
    """A special instance of a category. It defines an element
    which is a part of an other object.
    """

# ===================================================================#
#   LOGIC INFERENCE                                                  #
# ===================================================================#

from simag.core._helpers import OrderedSet

class Inference(object):
    
    class InferNode(object):
        def __init__(self, nc, ants, const, rule):
            self.rule = rule
            self.const = const
            self.ants = tuple(nc)
            if hasattr(rule, 'var_order'):
                self.subs = {v:set() for v in rule.var_order}
            else:
                self.subs = {}
            for ant in ants:
                if issubclass(ant.__class__, LogFunction):
                    args = ant.get_args()
                    for v in args:
                        if v in self.subs: self.subs[v].add(ant.func)
                elif issubclass(ant.__class__, LogPredicate):
                    if ant.term in self.subs:
                        self.subs[ant.term].add(ant.parent)
    
    class NoSolutionError(Exception): pass
    
    def __new__(cls, *args, **kwargs):
        obj = super(Inference, cls).__new__(cls)
        obj.parser = logic_parser
        return obj
    
    def __init__(self, kb, *args, **kwargs):
        self._ignore_current = False
        if kwargs:
            for k, v in kwargs.items():
                if k == 'ignore_current':
                    self._ignore_current = v
        self.kb = kb
        self.vrs = set()
        self.nodes = OrderedDict()
        self.results = {}
        self.infer_facts(*args)

    def infer_facts(self, sent):
        """Inference function from first-order logic sentences.

        Gets a query from an ASK, encapsulates the query subtitutions, 
        processes it (including caching of partial results or tracking
        var substitution) and returns the answer to the query. If new 
        knowledge is produced then it's passed to an other procedure for
        addition to the KB.
        """        
        def query(pred, q):        
            # create a lookup table for memoizing results of previous passes
            if hasattr(self, 'queue') is False:
                self.queue = dict()
                for query in self.nodes.values():
                    for node in query: self.queue[node] = set()
            else:
                for node in self.queue: self.queue[node] = set()
            # Run the query, if there is no result and there is
            # an update, then rerun it again, else stop
            k, result, self._updated = True, None, list()
            while not result and k is True:                    
                chk, done = deque(), list()
                result = self.unify(q, chk, done)
                k = True if True in self._updated else False
                self._updated = list()
        
        self.get_query(sent)
        # Get relevant rules to infer the query
        self.rules, self.done = OrderedSet(), [None]
        while hasattr(self, 'ctgs'):
            try: self.get_rules()
            except Inference.NoSolutionError: pass
        # Get the caterogies for each individual/class
        self.obj_dic = self.kb.objs_by_ctg(self.chk_ctgs, 'individuals')
        klass_dic = self.kb.objs_by_ctg(self.chk_ctgs, 'classes')
        self.obj_dic.update(klass_dic)
        # Start inference process
        for var, preds in self.query.items():
            if var in self.vrs:
                for pred in preds:
                    if issubclass(pred.__class__, LogFunction): 
                        q, is_func = pred.func, True
                    elif issubclass(pred.__class__, LogPredicate): 
                        q, is_func = pred.parent, False   
                    for obj, ctgs in self.obj_dic.items():
                        result = None
                        if is_func:
                            # need to create a substitution
                            raise NotImplementedError
                            subst = None
                            if q in ctgs:              
                                result = self.kb.test_pred(subst, kls='func')
                        else:
                            subst = make_fact(pred, 'grounded_term',
                                from_free=True, **{'sbj': obj})
                            if q in ctgs:
                                result = self.kb.test_pred(subst, kls='pred')                      
                        if obj not in self.results: self.results[obj] = {}
                        if result is not None:
                            self.results[obj][q] = result
                        else:
                            # if no result was found from the kb directly
                            # make an inference from a grounded fact
                            self.actv_q = (subst.term, subst.parent, subst)
                            query(subst, self.actv_q[1])
            else:
                prev_res = self.results.setdefault(var,{})
                for pred in preds:
                    if issubclass(pred.__class__, LogFunction):
                        self.actv_q, q = (var, pred.func, pred), pred.func
                    elif issubclass(pred.__class__, LogPredicate):
                        self.actv_q, q = (var, pred.parent, pred), pred.parent
                    if q not in prev_res:
                        query(pred, q)
                    # if there is some unresolved query, add none to results
                    self.results[var].setdefault(q, None)
    
    def unify(self, p, chk, done):
        def add_ctg():
            # add category/function to the object dictionary
            # and to results dict if is the result for the query
            for r in proof_result:
                if issubclass(r.__class__, LogFunction):
                    args = r.get_args()
                    for obj in args:
                        self.obj_dic.setdefault(obj, set([r.func]))
                    if issubclass(pred.__class__, LogFunction):
                        try:
                            if pred == r:
                                self.results[query_obj][query] = True
                            else:
                                self.results[query_obj][query] = False
                        except NotCompFuncError: pass
                else:
                    cat, obj = r.parent, r.term
                    self.obj_dic.setdefault(obj, set([cat]))
                    if issubclass(pred.__class__, LogPredicate):
                        try:
                            if pred == r:
                                self.results[query_obj][query] = True
                            else:
                                self.results[query_obj][query] = False
                        except NotCompAssertError: pass
        
        query_obj, query = self.actv_q[0], self.actv_q[1]
        pred = self.actv_q[2]
        # for each node in the subtitution tree unifify variables
        # and try every possible substitution
        if p in self.nodes:
            # the node for each rule is stored in an efficient sorted list
            # by rule creation datetime, from oldest to newest, we iterate 
            # from newest to oldest as the newest rules take precedence
            #iter_rules = reversed(self.nodes[p])
            for node in self.nodes[p]:
                # recursively try unifying all possible argument with the 
                # operating logic sentence
                # check what are the possible var substitutions
                mapped = self.map_vars(node)
                # permute and find every argument combination       
                mapped = list(itertools.product(*mapped))
                # run proof until a solution is found or there aren't more
                # combinations left to be tested
                proof_result = None
                while not proof_result and (len(mapped) > 0):
                    args = mapped.pop()
                    result_memoization = hash(args)
                    if result_memoization not in self.queue[node]:
                        proof_result = node.rule(self.kb, args)
                        if proof_result is not False and proof_result is not None:
                            self._updated.append(True)
                            add_ctg()
                            self.queue[node].add(result_memoization)                            
                if p not in done:
                    chk = deque(node.ants) + chk
        if query_obj in self.obj_dic and query in self.obj_dic[query_obj]:
            return True
        elif len(chk) > 0:
            done.append(p)
            p = chk.popleft()
            self.unify(p, chk, done)
    
    def map_vars(self, node):
        # TODO: check out what combinations can be roled out before 
        # attempting to solve it for performance gains
        
        # map values to variables for subtitution
        subs_num = len(node.subs)
        subactv = [set()] * subs_num
        for i, t in enumerate(node.subs.values()):
            y = len(t)
            for obj, s in self.obj_dic.items():
                x = len(s)            
                if x >= y:
                    r = s.intersection(t)
                    if len(r) == y:
                        subactv[i].add(obj)
        return subactv
    
    def get_query(self, sent):
        # TODO: support queries for the same function/predicate for the same obj
        def assert_memb(p):
            result = self.kb.test_pred(p, kls='pred')
            if result:
                D = self.results.setdefault(p.term, {})
                D[p.parent] = True
            else:    
                if p.term not in terms.keys():
                    terms[p.term] = [p]
                    ctgs.append(p.parent)
                else:
                    terms[p.term].append(p)
                    ctgs.append(p.parent)
        
        def assert_rel(p):
            result = self.kb.test_pred(p, kls='func')            
            ids = p.get_args()
            for obj in ids:
                if result:
                    D = self.results.setdefault(obj, {})
                    D[p.func] = True
                else:
                    if obj not in terms.keys():
                        terms[obj] = [p]
                    else:
                        terms[obj].append(p)
            if not result: ctgs.append(p.func)
        
        # for each query, first try to retrieve the result from the kb
        # if it fails, then add to the query list
        if type(sent) is str:
            query = self.parser(sent, tell=False)            
        elif issubclass(sent.__class__, LogFunction):
            if not self._ignore_current:
                result = self.kb.test_pred(sent, kls='func')
            else: result = None
            self.query = {}
            for obj in sent.get_args():
                if result:
                    self.results[obj] = {sent.func: True}
                self.query[obj] = [sent]
            self.ctgs = [sent.func]
            return
        elif issubclass(sent.__class__, LogPredicate):
            if isinstance(sent, FreeTerm):
                self.vrs.add(sent.term)
            elif not self._ignore_current:
                result = self.kb.test_pred(sent, kls='pred')
                if result:
                    self.results[sent.term] = {sent.parent: True}
            self.query = {sent.term: [sent]}
            self.ctgs = [sent.parent]
            return
        else:
            raise TypeError('argument type is not valid')
        
        terms, ctgs = {}, []
        for p in query.assert_rel: assert_rel(p)
        for p in query.assert_memb: assert_memb(p)
        for q in query.queries:
            self.vrs.update(q.var_order)
            for p in q.preds: assert_memb(p)
            for p in q.funcs: assert_rel(p)
        self.query, self.ctgs = terms, ctgs
    
    def get_rules(self):
        def mk_node(pos):
            # makes inference nodes for the evaluation
            atom_pred = sent.get_preds(branch=pos)
            for const in atom_pred:
                if issubclass(const.__class__, LogFunction):
                    pred = const.func
                else:
                    pred = const.parent
                node = self.InferNode(nc, preds, pred, sent)
                if node.const in self.nodes:
                    self.nodes[node.const].add(node)
                else:
                    self.nodes[node.const] = SortedListWithKey(
                        key=lambda x: x.rule.created)
                    self.nodes[node.const].add(node)
        
        if len(self.ctgs) > 0: c = self.ctgs.pop()
        else: c = None
        if c is not None:
            self.done.append(c)            
            try:
                chk_rules = OrderedSet(self.kb.classes[c].cog)
                chk_rules = chk_rules - self.rules
            except:
                raise Inference.NoSolutionError(c)
            for sent in chk_rules:
                preds = sent.get_preds()
                nc = []
                for y in preds:
                    if issubclass(y.__class__, LogPredicate):
                        nc.append(y.parent)
                    elif issubclass(y.__class__, LogFunction):
                        nc.append(y.func)
                mk_node('right')
                nc2 = [e for e in nc if e not in self.done and e not in self.ctgs]
                self.ctgs.extend(nc2)
                if c in nc:
                    preds = sent.get_preds(branch='right')
                    nc = []
                    for y in preds:
                        if issubclass(y.__class__,LogPredicate): 
                            nc.append(y.parent)
                        elif issubclass(y.__class__, LogFunction):
                            nc.append(y.func)
                    mk_node('left')
                    nc2 = [e for e in nc if e not in self.done \
                           and e not in self.ctgs]
                    self.ctgs.extend(nc2)
            self.rules = self.rules | chk_rules
            self.get_rules()
        else:
            self.done.pop(0)
            self.chk_ctgs = set(self.done)
            del self.done
            del self.rules
            del self.ctgs
