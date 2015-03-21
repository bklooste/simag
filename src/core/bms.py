# -*- coding: utf-8 -*-

"""Belief Maintenance System for the agent system.

This module adds methods and classes for:
1) Recording how a belief came to existence to the agent.
2) Detecting inconsistences between new and old beliefs.
3) Fixing those inconsitences.
"""
# ===================================================================#
#   Imports and globals
# ===================================================================#
import datetime

# ===================================================================#
#   Recording subsystem
# ===================================================================#


class BmsWrapper(object):
    """Acts as a wrapper for the Belief Maintenance System for a given
    agent. 
    
    Converts from/to data in the database and serves to keep the
    believes alive in memory.
    """
    class WrappDecl(object):
        def __init__(self, parent):
            self.parent = parent

        def remake(self, sets):
            if len(sets) == 3:
                s = ['<', sets[0], '[', sets[1][0][0], ',u=', 
                     str(sets[1][0][1]), ';', sets[1][1], ']>']
            else:
                s = [sets[0], '[', sets[1][0], ',u=', str(sets[1][1]), ']']
            s = ''.join(s)
            self.parent.container[s] = {'form': 'SELF',
                                        'prev': None,
                                        'date': datetime.datetime.utcnow()}

    def __init__(self, ag):
        self.container = {}
        self.ag = ag

    def register(self, form, stop=False):
        if stop is True:
            self.wrk_bel.save()
        else:
            self.wrk_bel = BeliefRecord(form, self)

    def add(self, *args):
        if args[1] is True:
            self.wrk_bel = self.WrappDecl(self)
            self.check(args[0])

    def prev_blf(self, *args):
        self.wrk_bel.prev_blf(*args)

    def check(self, pred):
        """Initialises the sequence to detect inconsistencies between new
        beliefs and old beliefs.
        """
        if len(pred) == 2:
            if isinstance(pred[1][1], str):
                cat, sbj = pred[0], pred[1][0]
                val, op = float(pred[1][1][1:]), pred[1][1][0]
            else:
                cat, val, sbj = pred[0], float(pred[1][1]), pred[1][0]
                op = '='
            try:
                categs = self.ag.individuals[sbj].get_cat()
            except:
                return False
            if cat in categs and val != categs[cat]:
                # Check if there is an inconsistency.
                if op is '>' and categs[cat] > val:
                    pass
                elif op is '<' and categs[cat] < val:
                    pass
                else:
                    print 'INCONSISTENCY', pred, categs           
                    # Inconsitency found between values.
            self.wrk_bel.remake(pred)
        elif len(pred) == 3:
            rel0, sbj, obj = pred[0], pred[1][1], pred[1][0][0]
            if isinstance(pred[1][0][1], str):
                val, op = float(pred[1][0][1][1:]), pred[1][0][1][0]
            else:
                val, op = pred[1][0][1], '='
            if sbj in self.ag.individuals and '$' in sbj \
            and rel0 in self.ag.individuals[sbj].relations:
                rel = self.ag.individuals[sbj].get_rel(rel0)
                if obj in rel and val != rel[obj]:
                    # Check if there is an inconsistency.
                    if op is '>' and rel[obj] > val:
                        pass
                    elif op is '<' and rel[obj] < val:
                        pass
                    else:
                        print 'INCONSISTENCY', rel0 + ': {\''+obj+'\': ' \
                        + str(val) + '}', rel
                        # Inconsitency found between values.
                        chk_const(self, pred, rel[obj])
            if not '$' in sbj:
                # It's the relation between a class and other object(s)
                pass
            self.wrk_bel.remake(pred)

class BeliefRecord(object):
    """Representation of how a belief become to existence.
    
    On initialisation it receives which belief came to existence,
    and the reference of the formula which produced the belief.
    """
    def __init__(self, form, wrapper):
        self.form = form
        self.bms = wrapper
        self.beliefs = []
        self.prod = []
    
    def remake(self, s):
        """Remakes the beliefs in a logic predicate form and stores them."""
        if len(s) == 2:
            symb, val = s[1][1][0], str(float(s[1][1][1:]))
            pred = ''.join([s[0],'[',s[1][0],',u',symb,val,']'])
        else:
            symb, val = s[1][0][1][0], str(float(s[1][0][1][1:]))
            pred = ''.join(['<',s[0],'[',s[1][0][0],',u',symb,val,
                            ';',s[1][1],']>'])
        self.beliefs.append(pred)

    def prev_blf(self, belief):
        """The belief may be a product of past beliefs, the possibility
        is explored and stores the immeate previous belief that 
        produced this belief.
        """
        if belief in self.bms.container:
            self.prod.append(belief)

    def save(self):
        if len(self.prod) != 0:
            for b in self.beliefs:
                self.bms.container[b] = {'form': self.form, 
                                         'prev': tuple(self.prod),
                                         'date': datetime.datetime.now()}
        else:
            for b in self.beliefs:
                self.bms.container[b] = {'form': self.form, 
                                         'prev': None,
                                         'date': datetime.datetime.now()}

# ===================================================================#
#   Maintenance subsystem
# ===================================================================#


def chk_const(bms, pred, pval):
    """Check what predicates are the cause of the inconsistency."""
    
    def k_chain(bms, chk_pred):
        """Reconstructs a logic sequence which represent the beliefs that
        produced an input belief.
        """
        print bms.container[chk_pred]['form']

    rel, sbj, obj = pred[0], pred[1][0], pred[1][1][0]
    opred = '<'+rel+'['+sbj+';'+obj+',u='+str(pval)+']>'
    if bms.container[opred]['form'] is 'SELF':
        # The fact has changed from the initial predicate
        # the initial fact being not True anymore.
        pass
    else:
        # Unpack the logic sequence.
        seq = []
        k_chain(bms, opred)
