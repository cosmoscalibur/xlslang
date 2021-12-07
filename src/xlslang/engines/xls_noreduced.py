from datetime import date
from decimal import Decimal

from arpeggio import PTNodeVisitor

from xlslang.engines.datatypes import XString

class XlsNoReducedVisitor(PTNodeVisitor):
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
        cum = children.x_factor[0]
        cum = cum * Decimal('0.01') if children.x_op_perc else cum
        if children.x_op_add and children.x_op_add[0] == '-':
            cum = -cum
        return cum

    def visit_x_power(self, node, children):
        cum = children.x_unary_ops[-1]
        if len(children.x_unary_ops) > 1:
            for pow in reversed(children.x_unary_ops[:-1]):
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
        result = children.x_expression[0]
        if not children.xcell_assigment:
            return result
        else:
            varname = children.xcell_assigment[0]
            self.variables[varname] = result

    def visit_x_code(self, node, children):
        output = children.xformula
        if len(output) == 1:
            return output[0], self.variables
        else:
            return output, self.variables

    def visit_xfunction_call(self, node, children):
        '''
        Crear firma de funciones.
        Módulo tipo constante con asociación de diccionario cuya clave es el
        nombre de la función y el valor tiene registro del import de la función,
        firma de argumentos (as-is, expand, custom), tipo de función (builtin, custom).
        La idea de una función builtin es no requerir resolución adicional
        '''
        pass
