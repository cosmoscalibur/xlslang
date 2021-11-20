from arpeggio import Optional, ZeroOrMore, OneOrMore, EOF
from arpeggio import RegExMatch as _

from xlslang.suppress_classes import SuppressStrMatch

# Data Types

def xinteger(): return _(r'-?\d+')
def xdecimal(): return _(r'-?\d+\.\d+')

def xdate(): return _(r'"\d{1,2}/\d{1,2}/\d{4}"')
def xstring(): return _(r'"[^"]*"')

def xboolean(): return _(r'(TRUE|FALSE)')

# Operators

def x_op_add(): return ['+', '-']
def x_op_mult(): return ['*', '/']
def x_op_pow(): return SuppressStrMatch('^')

def x_op_perc(): return SuppressStrMatch('%')
def x_op_concat(): return SuppressStrMatch('&')

def x_op_fix(): return '$'

def x_op_range(): return SuppressStrMatch(':')
# def x_op_space(): return ' '  # Intersection operator
def x_op_comma(): return SuppressStrMatch(',') #, Optional(x_op_space) # Union operator

def x_lparen(): return SuppressStrMatch('(')
def x_rparen(): return SuppressStrMatch(')')

def x_op_assign(): return '='

# References

def xcol_name(): return _(r'[A-Z]+')
def xsheet(): return _(r"(\w+|'[^']+')"), SuppressStrMatch('!')

def xcell(): return Optional(x_op_fix), xcol_name, Optional(x_op_fix), xinteger
def xsheet_cell(): return xsheet, xcell

def xrange_direct(): return xcell, x_op_range, xcell
# def xrange_intersection(): return xcell, OneOrMore(x_op_space, xcell)
# def xrange_union(): return xcell, OneOrMore(x_op_comma, xcell)
def xsheet_range(): return xsheet, xrange_direct

def xrange(): return [xsheet_range, xrange_direct] # xrange_union, xrange_intersection, 

# Functions

def x_arg(): return [xrange, x_factor]

def x_args(): return ZeroOrMore(x_arg, sep=x_op_comma) # Union operation

def xfunction_name(): return _(r'[A-Z]([A-Z]|_|[0-9])+')

def xfunction_call():
    return (
        xfunction_name,
        x_lparen,
        x_args,
        x_rparen
    )

# Operations
def xconcat(): return x_factor, x_op_concat, x_factor

def xperc(): return x_factor, x_op_perc

def x_unary_add(): return x_op_add, x_factor

def x_power(): return x_factor, x_op_pow, x_factor

def x_factor():
    return [
        xdecimal,
        xinteger,
        x_unary_add,
        xdate,
        xstring,
        xfunction_call,
        xboolean,
        xsheet_cell,
        xcell,
        x_pexpression,
    ]

def x_factor_arithm():
    return [
        xperc,
        x_factor,
        xconcat,
    ]

def x_term():
    return [
        x_power,
        (x_factor_arithm, ZeroOrMore(x_op_mult, x_factor_arithm, eolterm=True)),
    ]

def x_expression():
    return x_term, ZeroOrMore(x_op_add, x_term, eolterm=True)

def x_pexpression(): return x_lparen, x_expression, x_rparen

def xformula(): return x_op_assign, x_expression
def x_assigment(): return xcell, Optional(SuppressStrMatch('<-'), xfunction_name)
def x_line(): return Optional(x_assigment), xformula
def x_code(): return OneOrMore(x_line, sep=SuppressStrMatch('\n')), EOF
