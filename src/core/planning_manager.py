# -*- coding: utf-8 -*-

"""Planning Manager module.

This module manages the different planning implementations, the context 
for planning and the selection of the different algorithms based on context.


"""

# ===================================================================#
#   Imports and globals
# ===================================================================#

from types import MethodType, FunctionType

# ===================================================================#
#   CONTEXT MANAGER
# ===================================================================#

class Context:
    """Wrapps the context data and acts as an interface for the
    different problem sets. Decission is delegated then to an
    the strategy manager based on the problem set.
    """    
    pass

def context_manager():
    """Extracts the data from the current agent knowledge necessary
    for planning actions.
    
    Input -> agent object
    Output -> context object
    """
    pass

# ===================================================================#
#   PLANNING ALGORITHMS IMPLEMENTATIONS
# ===================================================================#
# 
# Planning algorithms are loaded based on the current context and
# problem case.

class ProblemMeta(type):
    """Manages the creation of 'ProblemDomain' subclasses.
    
    When a subclass of ProblemDomain is created, it's checked if the data 
    input interface is compatible with the context.
    
    It also checks if the output is compatible with the existing available 
    actions/choices to the agent, and the instructions are in a compatible 
    data structure.
    """
    __problems = list()
    def __call__(cls, *args, **kwargs):
        new_cls = super().__call__(*args, **kwargs)
        subs, subcls = False, False
        for i, pcls in enumerate(cls.__problems):
            if isinstance(pcls, cls):
                subs = True
                break
            if issubclass(cls, pcls.__class__):
                subs, subcls = True, True
                break
        if subcls is False:
            ProblemMeta.__check_input_data(cls)
            ProblemMeta.__check_output_data(cls)            
            if subs is True: cls.__problems[i] = new_cls
            else: cls.__problems.append(new_cls)
        return new_cls

    def __check_input_data(cls): pass
    
    def __check_output_data(cls): pass

class ProblemDomain(metaclass=ProblemMeta):
    """An interface to define the problems domain, its solution algorithms, 
    and transformation of the data from the context to solve the problem.
    
    When subclassed, it defines the 'problem context' upon which 
    the algorithms are selected for problem resolution and loaded as needed.
    
    When a subclass, representing a problem domain, is instantiated, can be 
    loaded with the different implementation algorithms to solve that 
    particular set of problems.
    """
    def __init__(self,
                 actions=None,
                 knowledge=None,
                 relations=None,
                 goal=None,
                 init=None):
        cls = self.__class__
        args = ['actions','knowledge','relations','goal','init']
        for attr in args:
            val = locals().get(attr)
            if not hasattr(cls, attr):
                setattr(cls, attr, val)
            elif val is not None:
                setattr(cls, attr, val)
            if getattr(cls, attr) is None:
                m = "Need to provide '{0}' argument.".format(attr)
                raise AttributeError(m)
    
    def __call__(self, agent, **kwargs):
        if hasattr(self, 'default'):
            self.agent = agent
            chk = self.inspect_domain()
            if chk is not None: raise chk
            # run the resolution algorithm
            if issubclass(self.default.__class__, SolveTemplate):
                self.default(agent, self, **kwargs)
            else:
                self.default(**kwargs)
        else: 
            raise AttributeError('Need to set default algorithm, ' \
            'use the set_default method.')

    def add_algo(self, *algos):
        for algo in algos:
            f = MethodType(algo, self)
            setattr(self, algo.__name__, f)
        
    def set_algo(self, func=None, subplans=None):
        if func is None: del self.default
        elif type(func) is FunctionType:
            if hasattr(self, func.__name__):
                self.default = getattr(self, func.__name__)
            else:
                self.add_algo(func)
                self.default = getattr(self, func.__name__)
        elif type(func) is type:
            if subplans is not None: self.default = func(subplans=subplans)
            else: self.default = func()
        else:
            self.default = func
            
    def inspect_domain(self, init=False):
        """Inspects the problem domain definition and continues 
        if there isn't any incompatibility problem found."""
        # check initial conditions of the problem
        for cond in self.__class__.init:
            if self.agent.ask(cond, single=True) is False:
                err = ValueError("The initial condition '{0}' is not " \
                "present right now.".format(cond))
                return err
        if init is False:
            # check if the required agent actions exists
            for action in self.__class__.actions:
                if action not in self.agent.actions:
                    err = AttributeError("The agent {0} doesn't have " \
                    "the required action.".format(self.agent))
                    return err
            # check if the a priori knowledge and relations exist
            for relation in self.__class__.relations:
                if self.agent.has_relation(relation) is False:
                    err = AttributeError("The agent {0} doesn't have " \
                    "the required relation.".format(self.agent))
                    return err
            for cog in self.__class__.knowledge:
                if self.agent.has_knowledge(cog) is False:
                    err = AttributeError("The agent {0} doesn't have " \
                    "the required knowledge.".format(self.agent))
                    return err
    
    def require_relations(self, relations):
        cls = self.__class__
        for rel in relations:
            if rel not in cls.relations:
                cls.relations.append(rel)
    
    def require_knowledge(self, knowledge):
        cls = self.__class__
        for cog in knowledge:
            if cog not in cls.knowledge:
                cls.relations.append(cog)
                
    def __str__(self):
        return '<'+str(self.__class__.__name__)+'>'

class SolveTemplate:
    """A helper template class for constructing resolution algorithms.
    
    This class includes several methods that can be executed by the algorithm:
    :method: observe -> Calls the agent perceived state and returns whether
    the query is true or false, this is useful if the previously perceived 
    state of the world needs to be updated in case it must be re-evaluated.
    :method: review -> It reviews if the current plan still is valid and
    will reach the goal. Useful to call after a critical action (with 
    unforeseen consequences) has been executed, for example.
    :method: call_plan -> Starts the execution of a new (sub)plan.
    :method: solve -> Call to start the execution of the algorithm.
    
    Those four methods allow for the building of increasingly complex plans
    while retaining the flexibility to return control to the agent.
    """
    def __init__(self, subplans=None):
        if subplans is not None: 
            self.subplans = subplans
    
    def __call__(self, agent, problem, **kwargs):
        self.agent = agent
        self.masterplan = problem
        self.solve(**kwargs)
        
    def observe(self, *args):
        self.agent.ask(*args)
        
    def review(self):
        # is the goal reachable in the current conditions?
        self.masterplan.inspect_domain(init=True)
    
    def call_plan(self, plan, subplans=None, **kwargs):
        if hasattr(self, 'subplans') and plan in self.subplans:
            plan_instance = plan(subplans)
            res = plan_instance(self.agent, self.masterplan, **kwargs)
            return res
        else: 
            raise ValueError("Plan not available in self.subplans.")
    
    def solve(self, *args, **kwargs):
        m = """Need to define the 'solve' function for the algorithm 
           '{1}' of problem '{0}'.""".format(self.masterplan, self)
        raise TypeError(m)
    
    def __str__(self):
        return '<'+str(self.__class__.__name__)+'>'

#=============================================================#

class FakeAgent(object):
    
    def __init__(self):
        self.actions = {'fake_action1':True, 'fake_action2':True}
        
    def has_relation(self, *args): return True
    
    def has_knowledge(self, *args): return True
    
    def ask(self, sent, single=False): return True        
    
    def action(self, act): 
        if act in self.action: return True

class ExampleProblem1(ProblemDomain):
    relations = ['relations ag should have']
    knowledge = ['knowledge ag should have']
    actions = ['fake_action1', 'fake_action2']
    goal = ['<on[table,box]>']
    init = ['this would be the initial conditions']
    
class SolveProblemWithAlgo1(SolveTemplate):    
    def solve(self, **kw):
        m = "attempting solution with algo {0} to problem {1}:\n" \
        "{2}\n".format(self,self.masterplan,kw['test'])
        print(m)
        self.call_plan(SolveProblemWithAlgo2, subplans=[SolveProblemWithAlgo3])

class SolveProblemWithAlgo2(SolveTemplate):
    def solve(self):
        m = "attempting solution with algo {0} to problem {1}\n" \
        "".format(self,self.masterplan)
        print(m)
        self.call_plan(SolveProblemWithAlgo3, test=test)

class SolveProblemWithAlgo3(SolveTemplate):
    def solve(self,**kw):
        m = "attempting solution with algo {0} to problem {1}\n" \
        "{2}\n".format(self,self.masterplan,kw['test'])
        print(m)

agent = FakeAgent()
test = 'THIS IS A SAMPLE OF A TEST'
#
problem1 = ExampleProblem1()
problem1.set_algo(SolveProblemWithAlgo1,subplans=[SolveProblemWithAlgo2])
problem1.add_algo()
problem1(agent, test=test)

