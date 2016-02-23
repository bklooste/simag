# -*- coding: utf-8 -*-

# ===================================================================#
#   Imports and globals
# ===================================================================#

import copy
import datetime
import re

from simag.core.grammar.grako_parser import SIMAGParser, SIMAGSemantics

__all__ = (
'logic_parser',
# Types:
'LogFunction', 
'LogPredicate',
'LogSentence',
)

# ===================================================================#
#   LOGIC SENTENCE PARSER
# ===================================================================#

class Semantics(SIMAGSemantics):
    __reserved_words = ['var', 'exists', 'timeCalc']

parser = SIMAGParser()

class ParserState(object):
    __instance = None
    def __new__(cls):
        if ParserState.__instance is None:
            ParserState.__instance = object.__new__(cls)
        ParserState.__instance._state = 'tell'
        return ParserState.__instance
    
    @property
    def state(self):
        return self._state
    
    @state.setter
    def state(self, val):
        self._state = val

parser_eval = ParserState()

class ParseResults(object):        
    def __init__(self):
        self.assert_memb = []
        self.assert_rel = []
        self.assert_rules = []
        self.assert_cogs = []

class EmptyString(Exception):
    "Empty stream is not allowed"
    
def logic_parser(string, tell=True):
    """Takes a string and returns the corresponding structured representing
    object program for the logic function. It can parse several statements 
    at the same time, separated by newlines and/or curly braces. It includes 
    a scanner and parser for the synthatical analysis which translate to the
    `program` in form of an object.
    
    The parser is generated automatically through Grako:
    `https://pypi.python.org/pypi/grako/`
    """    
    if string is "" or string is None:
        raise EmptyString
    if tell is False: parser_eval.state = 'ask'
    else: parser_eval.state = 'tell'
    ast = parser.parse(string, rule_name='block')
    results = ParseResults()
    #import json
    #print(json.dumps(ast, indent=2))    
    for stmt in ast:
        if stmt.stmt is not None:
            sent = make_logic_sent(stmt.stmt)            
            results.assert_cogs.append(sent)
        elif stmt.rule is not None:
            sent = make_logic_sent(stmt)
            results.assert_rules.append(sent)
        elif stmt.assertion is not None:
            for assertion in stmt.assertion:
                if assertion.klass is not None:
                    if parser_eval.state == 'tell':
                        memb = make_fact(assertion, 'grounded_term')
                    else:
                        memb = make_fact(assertion, 'free_term')
                    results.assert_memb.append(memb)
                else:
                    func = make_function(assertion, 'relation')
                    results.assert_rel.append(func)
    return results

class LogSentence(object):
    """Object to store a first-order logic complex sentence.

    This sentence is the result of parsing a sentence and encode
    it in an usable form for the agent to classify and reason about
    objects and relations, cannot be instantiated directly.
    
    It's callable when instantiated, accepts as arguments:
    1) the working knowledge-base
    2) n strins which will subsitute the variables in the sentence
       or a list of string.
    """
    def __init__(self):
        self.depth = 0
        self.particles = []
    
    def __call__(self, ag, *args):
        self.ag = ag
        if type(args[0]) is tuple or type(args[0] is list):
            args = args[0]
        # Clean up previous results.
        self.assigned = {}
        if hasattr(self, 'var_types'):
            preds = [p for p in self.particles if p.cond == ':predicate:']
            self.assigned.update(
                { p.pred.date_var: p for p in preds if hasattr(p.pred, 'date_var') }
            )
        if hasattr(self, 'pre_assigned'):
            for key, val in self.pre_assigned.items():
                self.assigned[key] = val        
        self.cln_res()
        if not hasattr(self, 'var_order'):
            preds = self.get_preds(branch='r')
            ag.thread_manager(self.get_preds(branch='r'))
            self.start.solve_proof(self)
            ag.thread_manager(preds, unlock=True)
        elif hasattr(self, 'var_order') \
        and len(self.var_order) == len(args):
            # Check the properties/classes an obj belongs to
            for n, const in enumerate(args):
                if const not in ag.individuals: return
                var_name = self.var_order[n]
                # Assign an entity to a variable by order.
                if hasattr(self, 'var_types') and var_name in self.var_types \
                and var_name not in self.assigned.keys():
                    type_ = self.var_types[var_name]
                    assert isinstance(const, type_), \
                    "{0} is not a {1} object".format(const, type_)
                self.assigned[var_name] = const
            # acquire lock
            preds = self.get_preds(branch='r')
            ag.thread_manager(preds)
            self.start.solve_proof(self)
            ag.thread_manager(preds, unlock=True)
        del self.ag
        if hasattr(self, 'result'): 
            result = self.result
            del self.result
            return result
        
    def get_ops(self, p, chk_op=['||', '=>', '<=>']):
        ops = []
        for p in self:
            if any(x in p.cond for x in chk_op):
                ops.append(p)
        for p in ops:
            x = p
            while x.cond != '|>' or x.parent == -1:
                if x.parent.cond == '|>' and x.parent.next[1] == x:
                    return False
                else:
                    x = x.parent
        return True
    
    def get_all_preds(self):
        preds = []
        for p in self:
            if p.cond == ':predicate:':
                preds.append(p.pred)
        return preds
    
    def get_preds(self, branch='l', conds=('|>', '=>', '<=>')):
        if self.start.cond not in conds:
            return self.get_all_preds()
        all_pred = []
        for p in self:
            if p.cond == ':predicate:':
                all_pred.append(p)
        preds = []
        for p in all_pred:
            top, bottom = p.parent, p
            while top.cond not in conds and top.parent:
                top, bottom = top.parent, top
            if branch == 'l' and top.next[0] is bottom:
                preds.append(p.pred)
            elif branch != 'l' and top.next[1] is bottom:
                preds.append(p.pred)
        return preds
    
    def cln_res(self):
        for particle in self.particles: del particle.results
    
    def __iter__(self):
        return iter(self.particles)
    
    def __repr__(self):
        def next_lvl_repr(obj):
            if hasattr(obj, 'next'):
                lhs = next_lvl_repr(obj.next[0])
                rhs = next_lvl_repr(obj.next[1])
                r = "".join(('{', lhs, ' ', obj.cond, ' ', rhs, '}'))
            else:
                r = ' ' + repr(obj) + ' '
            return r
        rep = next_lvl_repr(self.start)
        return rep
        
def make_logic_sent(ast):
    """Takes a parsed FOL sentence and creates an object with
    the embedded methods to resolve it.
    """
    
    class Particle(object):
        """This is the base class to create logic particles
        which pertain to a given sentence.
        
        Keys for solving proofs:
        100: Substitute a child's predicates.
        101: Check the truthiness of a child atom.
        103: Return to parent atom.
        """
        def __init__(self, cond, depth, parent, *args):
            self.depth = depth
            self.cond = cond
            self.parent = parent
            self._results = []
            if cond == ':predicate:':
                self.pred = args[0]
            else:
                self.next = []
    
        def __str__(self):
            if self.cond != ':predicate:':
                s = "<operator {1} (depth: {0})>".format(
                    str(self.depth), self.cond)
            else:
                s = "<predicate {1} (depth: {0})>".format(
                    str(self.depth), self.cond)
            return s
        
        def __repr__(self):
            if self.cond != ':predicate:':
                s = "<operator '{}' (depth: {})>".format(
                    self.cond, self.depth)
            else:
                s = "<predicate '{}'>".format(self.pred)
            return s
        
        @property
        def results(self):
            return self._result
        
        @results.deleter
        def results(self):
            self._results = []
    
        def return_value(self, truth, *args):
            self._results.append(truth)
        
        def substitute(self, proof, *args):
            raise AssertionError("operators of the type `{0}` can't be on " \
            + "the left side of the logic sentence".format(self.cond))
    
    class LogicIndCond(Particle):
        
        def solve_proof(self, proof):
            self.next[0].resolve(proof)
            if self._results[0] is True:
                self.next[1].substitute(proof)
            elif self._results[0] is False:
                if not hasattr(proof, 'result'):
                    proof.result = False
        
        def resolve(self, proof, *args):
            raise AssertionError("indicative conditional type " \
            + "arguments can only contain other indicative conditional " \
            + "or assertions in their right hand side")
        
        def substitute(self, proof, *args):
            if len(self._results) == 0:
                self.next[0].resolve(proof, *args)
            if self._results[0] is True:
                self.next[1].substitute(proof, *args)
                self.parent.return_value(True)
            else:
                self.parent.return_value(False)
            
    class LogicEquivalence(Particle):
        
        def solve_proof(self, proof):
            for p in self.next:
                p.resolve(proof)
            if any([True for x in self._results if x is None]):
                proof.result = None
            elif self._results[0] == self._results[1]:
                proof.result = True
            else:
                proof.result = False
        
        def resolve(self, proof, *args):
            for p in self.next:
                p.resolve(proof)
            if any([True for x in self._results if x is None]):
                self.parent.return_value(None)
            elif self._results[0] == self._results[1]:
                self.parent.return_value(True)
            else:
                self.parent.return_value(False)
    
    class LogicImplication(Particle):
        
        def solve_proof(self, proof):
            for p in self.next:
                p.resolve(proof)
            if (self._results[0] and self._results[1]) is None:
                proof.result = None
            else:
                if self._results[1] is False and self._results[0] is True:
                    proof.result = False
                else:
                    proof.result = True
        
        def resolve(self, proof, *args):
            for p in self.next:
                p.resolve(proof, *args)
            if (self._results[0] and self._results[1]) is None:
                self.parent.return_value(None)
            else:
                if self._results[1] is False and self._results[0] is True:
                    self.parent.return_value(False)
                else:
                    self.parent.return_value(True)
    
    class LogicConjunction(Particle):
        
        def solve_proof(self, proof):
            for p in self.next:
                p.resolve(proof)
            if all(self._results) and len(self._results) > 0:
                proof.result = True
            elif False in self._results:
                proof.result = False
        
        def resolve(self, proof, *args):
            for p in self.next:
                p.resolve(proof, *args)
            if all(self._results) and len(self._results) > 0:
                self.parent.return_value(True)
            elif False in self._results:
                self.parent.return_value(False)
            else:
                self.parent.return_value(None)
        
        def substitute(self, proof, *args):
            for p in self.next:
                p.substitute(proof, *args)
    
    class LogicDisjunction(Particle):
        
        def solve_proof(self, proof):
            for p in self.next:
                p.resolve(proof)
            if any(self._results) and len(self._results) > 0:
                proof.result = True
            elif all([True for x in self._results if x is False]):
                proof.result = False
        
        def resolve(self, proof, *args):
            for p in self.next:
                p.resolve(proof, *args)
            if any(self._results) and len(self._results) > 0:
                self.parent.return_value(True)
            elif all([True for x in self._results if x is False]):
                self.parent.return_value(False)
            else:
                self.parent.return_value(None)
        
    class LogicAtom(Particle):
        
        def resolve(self, proof):
            ag = proof.ag
            result = None
            if issubclass(self.pred.__class__, LogFunction):
                # Check funct between a set/entity and other set/entity.
                args = self.pred.get_args()
                for x, arg in enumerate(args):
                    if arg in proof.assigned:
                        args[x] = proof.assigned[arg]
                test = self.pred.substitute(args)
                if '$' in args[0][0]:
                    result = ag.individuals[args[0]].test_rel(test)
                else:
                    result = ag.classes[args[0]].test_rel(test)
            elif issubclass(self.pred.__class__, LogPredicate):
                # Check membership to a set of an entity.
                sbj = self.isvar(self.pred.term, proof)
                test = self.pred.substitute(sbj)
                if '$' not in sbj[0]:
                    try: 
                        result = ag.classes[sbj].test_ctg(test)
                    except KeyError:
                        result = None
                else:
                    try:
                        result = ag.individuals[sbj].test_ctg(test)
                    except KeyError:
                        result = None
            else:
                # special function types
                if type(self.pred) == TimeFunc:
                    dates = {}
                    for arg, p in proof.assigned.items():
                        if arg in self.pred.args and type(p) is not str:
                            dates[arg] = p.get_date(proof, ag)
                        elif arg in self.pred.args:
                            dates[arg] = p
                    if None not in dates.values():
                        test = self.pred.substitute(dates)
                        if test: result = True
                        else: result = False
            self.parent.return_value(result)
        
        def substitute(self, proof):
            ag = proof.ag
            # add subtitute var(s) for constants
            # and pass to agent for declaration
            if issubclass(self.pred.__class__, LogFunction):
                args = self.pred.get_args()
                for x, arg in enumerate(args):
                    if arg in proof.assigned:
                        args[x] = proof.assigned[arg]
                pred = self.pred.substitute(args)
                ag.bmsWrapper.add(pred, proof)
                ag.up_rel(pred)
            elif issubclass(self.pred.__class__, LogPredicate):
                sbj = self.isvar(self.pred.term, proof)
                pred = self.pred.substitute(sbj, val=None, ground=True)
                ag.bmsWrapper.add(pred, proof)
                ag.up_memb(pred)
            if hasattr(proof, 'result'):
                proof.result.append(pred)
            else:
                proof.result = [pred]
        
        def isvar(self, term, proof):
            try: term = proof.assigned[term]
            except KeyError: pass
            return term
        
        def get_date(self, proof, ag):
            date = None
            if issubclass(self.pred.__class__, LogFunction):
                args = self.pred.get_args()
                for x, arg in enumerate(args):
                    if arg in proof.assigned:
                        args[x] = proof.assigned[arg]
                test = self.pred.substitute(args)
                if '$' in args[0][0]:
                    date = ag.individuals[args[0]].get_date(test)
                else:
                    date = ag.classes[args[0]].get_date(test)
            elif issubclass(self.pred.__class__, LogPredicate):
                try:
                    sbj = proof.assigned[self.pred.term]
                    test = self.pred.substitute(sbj)
                except KeyError:
                    test = self.pred
                if '$' in test.term[0]:
                    date = ag.individuals[test.term].get_date(test)
                else:
                    date = ag.classes[test.term].get_date(test)
            return date
        
        def __repr__(self):
            return repr(self.pred)
        
    def new_atom(form, depth, parent, part_id, syb):
        VAR_DECLARATION = ('var', 'exists')
        if any(x in form for x in VAR_DECLARATION):
            cond = ':stub:'
            for s in VAR_DECLARATION:
                pos = form.find(s)
                if pos != -1: break
            cl = form.find(';')
            vars_ = form[pos+len(s):cl].replace(' ','').split(',')            
            for var in vars_:
                if ':' in var:
                    var = var.split(':')
                    if not hasattr(sent, 'var_types'):
                        sent.var_types = dict()
                    sent.var_types[var[0]], val = _get_type_class(var[1])
                    if val is not None:
                        if not hasattr(sent, 'pre_assigned'):
                            sent.pre_assigned = {}
                        sent.pre_assigned[var[0]] = val
                elif var not in sent.var_order:
                    sent.var_order.append(var)
            p = Particle(cond, depth, part_id, parent, syb, form)
            sent.particles.append(p)
        elif '[' in form:
            cond = ':predicate:'
            result = _check_reserved_words(form)
            if result == 'time_calc':
                form = make_function(form, 'time_calc')
            elif ('<' and '>') in form and '<=>' not in form:
                form = make_function(form, 'relation')
            else:
                form = make_fact(form, 'free_term')
            p = LogicAtom(cond, depth, part_id, parent, syb, form)
            sent.particles.append(p)
    
    def traverse_ast(remain, parent, depth):        
        def get_type(stmt, cmpd=False):            
            if stmt.func is not None:
                if cmpd is True:
                    return make_function(stmt, 'relation')
                return make_function(stmt.func, 'relation')
            elif stmt.klass is not None:
                if cmpd is True:
                    return make_fact(stmt, 'free_term')
                return make_fact(stmt.klass, 'free_term')
        
        def cmpd_stmt(preds, depth, parent):
            pred = preds.pop(0)
            if len(preds) > 1:
                form = get_type(pred, cmpd=True)
                particle = LogicAtom(':predicate:', depth, parent, form)          
                new_node = LogicConjunction('&&', depth, parent)
                parent.next.extend((new_node, particle))
                sent.particles.extend((new_node, particle))
                cmpd_stmt(preds, depth+1, new_node)                
            elif len(preds) == 1:
                form = get_type(pred, cmpd=True)
                particle1 = LogicAtom(':predicate:', depth, parent, form)
                form = get_type(preds.pop(0), cmpd=True)
                particle2 = LogicAtom(':predicate:', depth, parent, form)
                parent.next.extend((particle1, particle2))
                sent.particles.extend((particle1, particle2))
        
        form = get_type(remain)
        if form is not None:
            particle = LogicAtom(':predicate:', depth, parent, form)
        elif remain.assertion is not None:
            particle = LogicConjunction('&&', depth, parent)
            cmpd_stmt(remain.assertion, depth+1, particle)
        else:
            if remain.op == '|>':
                particle = LogicIndCond(remain.op, depth, parent)
            elif remain.op == '<=>':
                particle = LogicEquivalence(remain.op, depth, parent)
            elif remain.op == '=>':
                particle = LogicImplication(remain.op, depth, parent)
            elif remain.op == '&&':
                particle = LogicConjunction(remain.op, depth, parent)
            elif remain.op == '||':
                particle = LogicDisjunction(remain.op, depth, parent)
        sent.particles.append(particle)
        if parent: parent.next.append(particle)
        else: sent.start = particle
        # traverse down
        if remain.lhs is not None:
            traverse_ast(remain.lhs, particle, depth+1)
        if remain.lhs is not None:
            traverse_ast(remain.rhs, particle, depth+1)
        if sent.depth < depth: sent.depth = depth
    
    sent = LogSentence() 
    if ast.vars is not None:
        sent.var_order = ast.vars
    if ast.skol is not None:
        sent.skol_vars.extend(ast.skol)
    depth, parent = 0, None
    if ast.expr:
        traverse_ast(ast.expr, parent, depth)
    elif ast.rule:
        traverse_ast(ast.rule, parent, depth)
    return sent

# ===================================================================#
#   LOGIC CLASSES AND SUBCLASSES
# ===================================================================#


class MetaForAtoms(type):
    
    def __new__(cls, name, bases, attrs, **kwargs):
        attrs['_eval_time_truth'] = MetaForAtoms.__eval_time_truth        
        # Add methods from LogFunction to TimeFunc and store it at one location
        if name == 'TimeFunc':
            if not hasattr(MetaForAtoms, 'TimeFunc'):
                from types import FunctionType
                for m in globals()['LogFunction'].__dict__.values():
                    if type(m) == FunctionType \
                    and m.__name__ not in attrs.keys():                    
                        attrs[m.__name__] = m
                MetaForAtoms.TimeFunc = super().__new__(cls, name, bases, attrs)
            return MetaForAtoms.TimeFunc
        # Store FreeTerm and return from same memory address
        elif name == 'FreeTerm':
            if not hasattr(MetaForAtoms, 'FreeTerm'):
                MetaForAtoms.FreeTerm = super().__new__(cls, name, bases, attrs)
            return MetaForAtoms.FreeTerm
        # return the new class
        return super().__new__(cls, name, bases, attrs)
    
    def __eval_time_truth(self, other):
        now = datetime.datetime.now()
        isTrueOther, isTrueSelf = True, True
        if hasattr(self, 'dates'):
            if (len(self.dates) % 2 or len(self.dates) == 1) \
            and self.dates[-1] < now: 
                isTrueSelf = True
            else: 
                isTrueSelf = False
        if hasattr(other, 'dates'):
            if (len(other.dates) % 2 or len(other.dates) == 1) \
            and other.dates[-1] < now: 
                isTrueOther = True
            else:
                isTrueOther = False
        # Compare truthiness of both
        if (isTrueOther and isTrueSelf) is True \
        or (isTrueOther and isTrueSelf) is False:
            return True
        else:
            return False

class LogPredicate(metaclass=MetaForAtoms):
    """Base class to represent a ground predicate."""
    types = ['grounded_term', 'free_term']
    
    def __init__(self, pred):
        #print(pred)
        arg = pred.args[0]
        val, op = float(arg.uval[1]), arg.uval[0]
        if (val > 1 or val < 0):
            m = "Illegal value: {0}, must be > 0, or < 1.".format(val[1])
            raise AssertionError(m)
        dates = None
        # optional arguments
        """
        dates = None
        if len(val) >= 3:
            for arg in val[2:]:
                if '*t' in arg:     
                    date = _set_date(arg)
                    if type(date) is datetime.datetime:
                        if dates is None: dates = []
                        dates.append(date)
                    else: self.date_var = date
        """
        return pred.klass, arg.term, val, op, dates

    def change_params(self, new=None, revert=False):
        if revert is not True:
            self.oldTerm, self.term = self.term, new
        else:
            self.term = self.oldTerm
            del self.oldTerm
                
    def __repr__(self):
        return '{0}({1}: {2})'.format(
            self.__class__.__name__, self.parent, self.term)

def make_fact(pred, f_type=None, **kwargs):
    """Parses a grounded predicate and returns a 'fact'."""
    
    class GroundedTerm(LogPredicate):
        
        def __init__(self, pred, fromfree=False, **kwargs):
            if fromfree is True:
                assert type(pred) == make_fact(None, 'free_term'), \
                    'The object is not of <FreeTerm> type'
                for name, value in pred.__dict__.items():
                    setattr(self, name, value)
                self.term = kwargs['sbj']
                if hasattr(self, 'date_var'): del self.date_var
            else:
                parent, term, val, op, dates = super().__init__(pred)
                self.parent = parent
                self.term = term                
                assert (op == '='), \
                "It's a grounded predicate, must assign truth value."
                self.op = op
                if (val > 1 or val < 0):
                    m = "Illegal value: {0}, must be > 0, or < 1." .format(val)
                    raise AssertionError(m)
                self.value = val
                if dates is not None:
                    self.dates = dates
        
        def __eq__(self, other):
            # Test if the statements are true at this moment in time
            time_truth = self._eval_time_truth(other)
            if time_truth is False: return False
            # test against other            
            if other.parent == self.parent \
            and other.term == self.term:
                return True
            else: return False
    
    class FreeTerm(LogPredicate):
        
        def __init__(self, pred):
            parent, term, val, op, dates = super().__init__(pred)
            self.parent = parent
            self.term = term
            self.value = val
            self.op = op
            if dates is not None:
                self.dates = dates
        
        def __eq__(self, other):
            # Test if the statements are ture at this moment in time
            time_truth = self._eval_time_truth(other)
            if time_truth is False: return False
            # test against other
            if not issubclass(other.__class__, LogPredicate):
                m = "{0} and {1} are not comparable.".format(other, LogPredicate)
                raise TypeError(m)
            if self.op == '=' and other.value == self.value:
                return True
            elif self.op == '>' and other.value > self.value:
                return True
            elif self.op == '<' and other.value < self.value:
                return True
            else: return False
        
        def substitute(self, sbj=None, val=None, ground=False):
            if ground is True:
                return make_fact(
                    self, 
                    f_type='grounded_term', 
                    **{'fromfree': True, 'sbj':sbj})
            else:
                subs = copy.deepcopy(self)
                if sbj is not None: subs.term = sbj
                if val is not None: subs.value = val            
                return subs
    
    assert (f_type in LogPredicate.types or f_type is None), \
            'Function {0} does not exist.'.format(f_type)
    if f_type == 'grounded_term': 
        if pred is None: return GroundedTerm
        return GroundedTerm(pred, **kwargs)
    elif f_type == 'free_term': 
        if pred is None: return FreeTerm
        return FreeTerm(pred)
    else:
        if pred is None: return LogPredicate 
        return LogPredicate(pred)


class LogFunction(metaclass=MetaForAtoms):
    """Base class to represent a logic function."""
    types = ['relation','time_calc']
    
    def __init__(self, sent):        
        self.func = sent.func
        dates, args_id, mk_args = None, list(), list()                 
        for a in sent.args:
            if a.uval is not None:
                val = float(a.uval[1])
                if (val > 1 or val < 0):
                    m = "Illegal value: {0}, must be > 0, or < 1." .format(val)
                    raise AssertionError(m)
                op = a.uval[0]           
                mk_args.append((a.term, val, op))
            else:
                mk_args.append(a.term)
            args_id.append(a.term)
        # optional arguments
        """    
        for param in arg:
            if '*t' in param:
                date = _set_date(param)
                if type(date) is datetime.datetime:
                    if dates is None: dates = []
                    dates.append(date)
                else: self.date_var = date
        """
        args_id = hash(tuple(args_id))
        arity = len(mk_args)
        return mk_args, args_id, arity, dates
    
    def get_args(self):
        ls = []
        for arg in self.args:
            if isinstance(arg, tuple):
                ls.append(arg[0])
            else:
                ls.append(arg)
        return ls
    
    def substitute(self, args):   
        subs = copy.deepcopy(self)
        subs.args_ID = hash(tuple(args))     
        if type(args) is dict:
            for x, arg in enumerate(subs.args):
                if isinstance(arg, tuple) and arg in args:
                    subs.args[x] = list(arg)
                    subs.args[x][0] = args[arg]
                    subs.args[x] = tuple(subs.args[x])
                elif arg in args:
                    subs.args[x] = args[arg]
        elif type(args) is list:
            for x, arg in enumerate(subs.args):
                if isinstance(arg, tuple):
                    subs.args[x] = list(arg)
                    subs.args[x][0] = args[x]
                    subs.args[x] = tuple(subs.args[x])
                else:
                    subs.args[x] = args[x]
        return subs
    
    def change_params(self, new=None, revert=False):
        if revert is False:
            self.oldTerm = self.args.copy()
            for x, arg in enumerate(self.args):
                if isinstance(arg, tuple):
                    self.args[x] = list(arg)
                    self.args[x][0] = new[x]
                    self.args[x] = tuple(self.args[x])
                else:
                    self.args[x] = new[x]
        else:
            self.term = self.oldTerm
            del self.oldTerm 
    
    def __str__(self):
        return '{0}({1}: {2})'.format(
            self.__class__.__name__, self.func, self.args)
        
    def __repr__(self):
        return '{0}({1}: {2})'.format(
            self.__class__.__name__, self.func, self.args)

def make_function(sent, f_type=None, *args):
    """Parses and makes a function of n-arity.
    
    Functions describe relations between objects (which can be instantiated
    or variables). This functions can have any number of arguments, the
    most common being the binary functions.
    
    This class is instantiated and provides a common interface for all the 
    function types, which are registered in this class. It acts as an 
    abstraction to hide the specific details from the clients.
    
    The types are subclasses and will implement the details and internal
    data structure for the function, but are not meant to be instantiated
    directly.
    """
        
    class NotCompFuncError(Exception):
        """Logic functions are not comparable exception."""
    
        def __init__(self, args):
            self.err, self.arg1, self.arg2 = args  
    
    class RelationFunc(LogFunction):
        
        def __init__(self, sent):
            mk_args, args_id, arity, dates = super().__init__(sent)
            self.args_ID = args_id
            self.args = mk_args
            self.arity = arity
            # relation functions can only have a truth value for the 
            # first argument which represent the object of the relation,
            # the second argument is the subject, and the optional third
            # is the indirect object
            self.value = mk_args[0][1]
            if dates is not None: self.dates = dates
        
        def __eq__(self, other):
            comparable = self.chk_args_eq(other)
            if comparable is not True:
                raise NotCompFuncError(comparable)
            # Check if both are equal
            for x, arg in enumerate(self.args):
                if isinstance(arg, tuple):
                    oarg = other.args[x]
                    if arg[2] == '=' and arg[1] != oarg[1]:  
                        result = False                      
                    elif arg[2] == '>'and arg[1] > oarg[1]:
                        result = False     
                    elif arg[2] == '<'and arg[1] < oarg[1]:  
                        result = False
                    else:
                        result = True
            # Test if the statements are ture at this moment in time
            time_truth = self._eval_time_truth(other)
            if time_truth is True and result is True: return True
            else: return False
        
        def __ne__(self, other):
            comparable = self.chk_args_eq(other)
            if comparable is not True:
                raise NotCompFuncError(comparable)            
            # Check if both are not equal
            for x, arg in enumerate(self.args):
                if isinstance(arg, tuple):
                    oarg = other.arg[x]
                    if arg[2] == '=' and arg[1] != oarg[1]:
                        result = True                      
                    elif arg[2] == '>'and arg[1] < oarg[1]:
                        result = True     
                    elif arg[2] == '<'and arg[1] > oarg[1]: 
                        result = True
                    else:
                        result = True
            # Test if the statements are true at this moment in time
            time_truth = self._eval_time_truth(other)
            if time_truth is False and result is False: return True
    
        def chk_args_eq(self, other):
            if other.arity != self.arity:
                return ('arity', other.arity, self.arity)
            if other.func != self.func:
                return ('function', other.func, self.func)
            for x, arg in enumerate(self.args):
                if isinstance(arg, tuple):
                    if other.args[x][0] != arg[0]:
                        return ('args', other.args[x][0], arg[0])
                else:
                    if other.args[x] != arg:
                        return ('args', other.args[x], arg)
            return True
    
    class TimeFunc(metaclass=MetaForAtoms):
        """A special case for time calculus, not considered a relation.        
        It's not a subclass of LogFunction.
        """
        
        def __init__(self, sent):
            sent = sent.replace(' ','')
            rgx_ob = re.compile(r'\b(.*?)\]')
            func = rgx_ob.findall(sent)[0].split('[')[1]
            op = [c for c in func if c in ['>','<','==']]
            if len(op) > 1 or len(op) == 0:
                raise ValueError('provide one operator')
            else:
                self.operator = op[0]
                self.args = func.split(self.operator)
        
        def __bool__(self):
            if self.operator == '<' and self.args[0] < self.args[1]:
                return True
            elif self.operator == '>' and self.args[0] > self.args[1]:
                return True
            elif self.operator == '=' and self.args[0] == self.args[1]:
                return True
            else: 
                return False
        
        def substitute(self, *args):
            subs = LogFunction.substitute(self, *args)
            for x, arg in enumerate(subs.args):
                if type(arg) is str: subs.args[x] = _set_date(arg)
            return subs
        
        def __str__(self):
            return "TimeFunc({0})".format(self.args)

    assert (f_type in LogFunction.types or f_type is None), \
            'Function {0} does not exist.'.format(f_type)
    if f_type == 'relation':
        if sent is None: return RelationFunc
        return RelationFunc(sent)
    if f_type == 'time_calc':
        if sent is None: return TimeFunc
        return TimeFunc(sent)
    else:
        if sent is None: return LogFunction
        return LogFunction(sent)

TimeFunc = make_function(None, 'time_calc')
GroundedTerm = make_fact(None, 'grounded_term')
FreeTerm = make_fact(None, 'free_term')

# ===================================================================#
#   HELPER FUNCTIONS
# ===================================================================#

def _check_reserved_words(sent):
    pred = sent.split('[')[0]
    if 'timeCalc' in pred: return 'time_calc'
    return False
    
def _set_date(date):
    # check if it's a variable name or special wildcard 'NOW'
    date = date.replace('*t=','').split('.')
    if len(date) == 1:
        if date[0] == 'NOW': return datetime.datetime.now()
        else: return date[0]
    # else make a new datetime object
    tb = ['year','month','day','hour','minute','second','microsecond']
    dobj = {}
    for i,p in enumerate(date):
        if 'tzinfo' not in p:
            dobj[ tb[i] ] = int(p)
    if 'year' not in dobj: raise ValueError('year not specified')
    if 'month' not in dobj: raise ValueError('month not specified')
    if 'day' not in dobj: raise ValueError('day not specified')
    if 'hour' not in dobj: dobj['hour'] = 0
    if 'minute' not in dobj: dobj['minute'] = 0
    if 'second' not in dobj: dobj['second'] = 0
    if 'microsecond' not in dobj: dobj['microsecond'] = 0
    if 'tzinfo' not in dobj: dobj['tzinfo'] = None
    return datetime.datetime(
        year=dobj['year'],
        month=dobj['month'],
        day=dobj['day'],
        hour=dobj['hour'],
        minute=dobj['minute'],
        second=dobj['second'],
        microsecond=dobj['microsecond'],
        tzinfo=dobj['tzinfo']
    )
    
def _get_type_class(var):
    # split the string to take the variable
    var = [c for c in var.split('=')]
    if len(var) > 1: val = var[1]
    else: val = None
    # check type of the var    
    if var[0] == 'time':
        type_ = make_function(None, f_type='time_calc')
    return type_, val
