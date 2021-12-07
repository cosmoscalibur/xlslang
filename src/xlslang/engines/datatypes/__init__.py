from decimal import Decimal

class XString:

    def __init__(self, text:str):
        self.value = text
        self._number = None

    def __str__(self):
        return self.value

    @property
    def number(self):
        if self._number is None:
            self._number = Decimal(self.value)
        return self._number

    def join(self, text_list):
        joined = self.value.join([text.value for text in text_list])
        return XString(joined)

    def __eq__(self, other) -> bool:
        if isinstance(other, XString):
            return self.value == other.value
        else:
            return False

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
