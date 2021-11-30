from decimal import Decimal

import pytest

from xlslang.parser import XlsLangParser

@pytest.fixture
def parser(scope="module"):
    return XlsLangParser()


class TestDataType:
    @pytest.mark.parametrize(
        'formula,result',
        [
            ('=8', 8),
            ('=351', 351),
        ]
    )
    def test_xinteger(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

    @pytest.mark.parametrize(
        'formula,result',
        [
            ('=0.21', Decimal('0.21')),
            ('=99.456', Decimal('99.456')),
        ]
    )
    def test_xdecimal(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

    @pytest.mark.parametrize(
        'formula,result',
        [
            ('="0.21"', '0.21'),
            ('=""', ''),
            ('="A5"', 'A5'),
            ('="Hola mundo"', 'Hola mundo'),
            ('="TODAY()"', 'TODAY()'),
        ]
    )
    def test_xstring(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

class TestOperators:
    @pytest.mark.parametrize(
        'formula,result',
        [
            ('=+2.5', Decimal('2.5')),
            ('=9%', Decimal('0.09')),
            ('=-3.5%', Decimal('-0.035')),
        ]
    )
    def test_x_unary_ops(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

    @pytest.mark.parametrize(
        'formula,result',
        [
            ('=2+1', 3),
            ('=49-5', 44),
            ('=0.3+17.7', 18),
            ('=9.2-10', Decimal('-0.8')),
            ('=2+1-8', -5),
            ('=49-5+15+5.5', Decimal('64.5')),
            ('=2.0+2-4+8-10-10+20.5', Decimal('8.5')),
            ('=0.1+0.1+0.1+0.1+0.1+0.1+0.1', Decimal('0.7')),
        ]
    )
    def test_x_sum(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

    @pytest.mark.parametrize(
        'formula,result',
        [
            ('=2*3', 6),
            ('=8*-1.5', -12),
            ('=-0.2*-0.5', Decimal('0.1')),
            ('=9/10', Decimal('0.9')),
            ('=4*3/2*4', 24),
            ('=2/4*-1.5/3', Decimal('-0.25')),
            ('=9/-3*2.5', Decimal('-7.5'))
        ]
    )
    def test_x_product(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

    @pytest.mark.parametrize(
        'formula,result',
        [
            ('="hola"&" "&"mundo"', 'hola mundo'),
            ('="Xls"&"Lang"', 'XlsLang'),
        ]
    )
    def test_x_concat(self, parser, formula, result):
        parser.parse_from_string(formula)
        assert parser.run() == result

    @pytest.mark.parametrize(
        'formula,result',
        [
            ('=4<3', False),
            ('=-4<3', True),
            ('=8>-12', True),
            ('=8>12', False),
            ('=0=0', True),
            ('=0=-2.5', False),
            ('=3<=3', True),
            ('=4<=3', False),
            ('=8>=8', True),
            ('=8>=12', False),
            ('=-2.5<>-2.5', False),
            ('=-2.5<>0', True),
            ('=8>2=5', False),
            ('=-2<=0=0', True),
        ]
    )
    def test_x_expression(self, parser, formula, result):
        "Expression is non terminal node associated with compare operators"
        parser.parse_from_string(formula)
        assert parser.run() == result
