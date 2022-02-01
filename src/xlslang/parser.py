import sys

from arpeggio import ParserPython, visit_parse_tree

from xlslang.grammar import x_code
from xlslang.engines import XlsNoReducedVisitor, XlsReducedVisitor


class XlsLangParser():
    def __init__(
        self,
        ignore_case=False,
        debug=False,
        reduce_tree=False,
        memoization=False,
    ):
        self.reduce_tree = reduce_tree
        self.parser = ParserPython(
            x_code,
            ws=' \t\r',
            ignore_case=ignore_case,
            debug=debug,
            reduce_tree=reduce_tree,
            memoization=memoization
        )

    def parse_from_string(self, expr:str, logs=False):
        self.parse_tree = self.parser.parse(expr)
        if logs:
            print("PARSER ELEMENTS\n", self.parse_tree)
            print("PARSER TREE\n", self.parse_tree.tree_str())

    def parse_from_file(self, source_file):
        pass

    def run(self, debug=False):
        visitor = (
            XlsReducedVisitor if self.reduce_tree else XlsNoReducedVisitor
        )
        return visit_parse_tree(self.parse_tree, visitor(debug=debug))

    def build_py(self):
        pass

    def build_js(self):
        pass

    def build_ast(self):
        pass


if __name__ == "__main__":
    parser = XlsLangParser(reduce_tree=False, debug=False)
    parser.parse_from_string(sys.argv[1])
    print('Resultado: ', str(parser.run(False)))
