from datetime import date
from decimal import Decimal

from arpeggio import PTNodeVisitor

from xlslang.engines.datatypes import XString
from xlslang.engines import functions as xlsfunctions

class XlsReducedVisitor(PTNodeVisitor):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.variables = dict()

    def visit_xinteger(self, node, children):
        return int(node.value)

    def visit_xdecimal(self, node, children):
        return Decimal(node.value)

    def visit_xdate(self, node, children):
        return date.fromisoformat(node.value[1:-1])

    def visit_xstring(self, node, children):
        return XString(node.value[1:-1])

    def visit_xboolean(self, node, children):
        return True if node.value == 'TRUE' else False

    def visit_xcell(self, node, children):
        return self.variables[node.value]

    def visit_x_unary_ops(self, node, children):
        if children[0] in ('+', '-'):
            has_sign = True
            cum = children[1]
        else:
            has_sign = False
            cum = children[0]
        cum = cum * Decimal('0.01') if children.x_op_perc else cum
        if has_sign and children[0] == '-':
            cum = -cum
        return cum

    def visit_x_power(self, node, children):
        cum = children[-1]
        if len(children) > 1:
            for pow in reversed(children[:-1]):
                cum = pow ** cum
        return cum

    def visit_x_concat(self, node, children):
        if len(children) > 1:
            concat = XString("").join(children)
        else:
            concat = children[0]
        return concat

    def visit_x_product(self, node, children):
        els = (len(children) - 1) // 2
        cum = children[0]
        for el in range(els):
            if children[el * 2 + 1][0] == '*':
                cum *= children[(el + 1) * 2]
            else:
                cum /= children[(el + 1) * 2] * Decimal('1')
        return cum

    def visit_x_sum(self, node, children):
        els = (len(children) - 1) // 2
        cum = children[0]
        for el in range(els):
            if children[el * 2 + 1][0] == '+':
                cum += children[(el + 1) * 2]
            else:
                cum -= children[(el + 1) * 2]
        return cum

    def visit_x_expression(self, node, children):
        childs = len(children)
        if childs == 1:
            cum = children[0]
        else:
            els = (childs - 1) // 2
            cum = True
            for el in range(els):
                op_ix = el * 2 + 1
                op = children[op_ix]
                lnode = children[op_ix - 1]
                rnode = children[op_ix + 1]
                if op == '=':
                    cum &= lnode == rnode
                elif op == '<':
                    cum &= lnode < rnode
                elif op == '<=':
                    cum &= lnode <= rnode
                elif op == '>':
                    cum &= lnode > rnode
                elif op == '>=':
                    cum &= lnode >= rnode
                else:  # op <>
                    cum &= lnode != rnode
                if not cum:
                    break
        return cum

    def visit_x_pexpression(self, node, children):
        return children[0]

    def visit_xformula(self, node, children):
        varname = children.xcell_assignment[0]
        self.variables[varname] = children[1]

    def visit_x_code(self, node, children):
        if len(children) == 1:
            return children[0], self.variables
        else:
            return children, self.variables

    def visit_xfunction_call(self, node, children):
        xls_function = getattr(
            xlsfunctions,
            children.xfunction_name[0].lower()
        )
        return xls_function(*children.x_args)

    def visit_xfunction_name(self, node, children):
        xls_function = getattr(
            xlsfunctions,
            node.value.lower()
        )
        return xls_function()
