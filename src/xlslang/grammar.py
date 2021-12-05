from arpeggio import Combine, EOF, OneOrMore, Optional, ZeroOrMore
from arpeggio import RegExMatch as _

from xlslang.suppress_classes import SuppressStrMatch

# Data Types

def xinteger(): return _(r'\d+')
def xdecimal(): return _(r'\d+\.\d+')

def xdate(): return _(r'"\d{4}-\d{1,2}-\d{1,2}"')
def xstring(): return _(r'"[^"]*"')

def xboolean(): return _(r'(TRUE|FALSE)')

# Operators

def x_op_add(): return ['+', '-']
def x_op_mult(): return ['*', '/']
def x_op_pow(): return SuppressStrMatch('^')
def x_op_perc(): return '%'

def x_op_comparison(): return _(r'<>|[<>]=?|=')

def x_op_concat(): return SuppressStrMatch('&')

def x_op_fix(): return SuppressStrMatch('$')
def x_op_range(): return SuppressStrMatch(':')
def x_op_comma(): return SuppressStrMatch(',') # Union operator

def x_lparen(): return SuppressStrMatch('(')
def x_rparen(): return SuppressStrMatch(')')

def x_op_assign(): return SuppressStrMatch('=')

# References

def xcol_name(): return _(r'[A-Z]+')
def xsheet(): return _(r"(\w+|'[^']+')"), SuppressStrMatch('!')

def xcell(): return Optional(x_op_fix), xcol_name, Optional(x_op_fix), xinteger
def xsheet_cell(): return xsheet, xcell

def xrange_direct(): return xcell, x_op_range, xcell
def xsheet_range(): return xsheet, xrange_direct
def xrange(): return [xsheet_range, xrange_direct]

# Functions

def x_arg(): return [xrange, x_expression]
def x_args(): return ZeroOrMore(x_arg, sep=x_op_comma)

def xfunction_name(): return _(r'[A-Z]([A-Z]|_|[0-9])+')
def xfunction_call():
    return (
        xfunction_name,
        x_lparen,
        x_args,
        x_rparen
    )

# Operations


def x_factor():
    return [
        xfunction_call,
        x_pexpression,
        xdecimal,
        xinteger,
        xdate,
        xstring,
        xboolean,
        xsheet_cell,
        xcell,
    ]

def x_unary_ops(): return Optional(x_op_add), x_factor, Optional(x_op_perc)

def x_power(): return ZeroOrMore(x_unary_ops, x_op_pow), x_unary_ops

def x_concat(): return x_power, ZeroOrMore(x_op_concat, x_power)

def x_product(): return x_concat, ZeroOrMore(x_op_mult, x_concat)

def x_sum(): return x_product, ZeroOrMore(x_op_add, x_product)

def x_expression(): return x_sum, ZeroOrMore(x_op_comparison, x_sum)

def x_pexpression(): return x_lparen, x_expression, x_rparen

def xformula(): return x_op_assign, x_expression
def x_assigment(): return xcell, Optional(SuppressStrMatch('<-'), xfunction_name)
def x_line(): return Optional(x_assigment), xformula
def x_code(): return OneOrMore(x_line, sep=SuppressStrMatch('\n')), EOF
