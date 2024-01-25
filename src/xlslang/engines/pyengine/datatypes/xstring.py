from decimal import Decimal

class XString:

    def __init__(self, text:str, implicit=False):
        self.data = text
        self._number = None
        self.implicit = implicit

    def __str__(self):
        return self.data

    def __repr__(self):
        delim = "\"" if self.implicit else "'"
        return f'{self.__class__.__name__}({delim}{self.data}{delim})'

    @property
    def number(self):
        return Decimal(self.data) if self.implicit else None

    def join(self, text_list):
        joined = self.data.join([text.data for text in text_list])
        return XString(joined)

    def __eq__(self, other) -> bool:
        return isinstance(other, XString) and self.data == other.data

    def __neg__(self) -> Decimal:
        return -self.number

    def __pos__(self) -> Decimal:
        return self.number

    def __abs__(self) -> Decimal:
        return abs(self.number)

    def __add__(self, other) -> Decimal:
        if isinstance(other, XString):
            other = other.number
        return self.number + other

    def __radd__(self, other) -> Decimal:
        return self.__add__(other)

    def __sub__(self, other) -> Decimal:
        if isinstance(other, XString):
            other = other.number
        return self.number - other

    def __rsub__(self, other) -> Decimal:
        return other + self.__neg__()

    def __mul__(self, other) -> Decimal:
        if isinstance(other, XString):
            other = other.number
        return self.number * other

    def __rmul__(self, other) -> Decimal:
        return self.__mul__(other)

    def __truediv__(self, other) -> Decimal:
        if isinstance(other, XString):
            other = other.number
        return self.number / other

    def __rtruediv__(self, other) -> Decimal:
        return self.__truediv__(other)

    def __mod__(self, other) -> Decimal:
        if isinstance(other, XString):
            other = other.number
        return self.number % other

    def __rmod__(self, other) -> Decimal:
        return self.__mod__(other)
