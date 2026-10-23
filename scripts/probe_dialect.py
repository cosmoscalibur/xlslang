#!/usr/bin/env python3
"""Write tests/assets/probe*.ods and circular*.{xlsx,ods}.

probe.ods: formulas whose result may differ between Excel and LibreOffice,
recalculated by LibreOffice. probe_regex.ods / probe_literal.ods: the same
formulas recalculated under other document search settings (regular
expressions, case-sensitive, partial-cell; literal). circular_*: iterative
calculation stored in the document. See docs/FIDELITY.md."""
import os, re, subprocess, tempfile, zipfile
import openpyxl
from openpyxl.workbook.properties import CalcProperties

OUT = os.path.join(os.path.dirname(__file__), "..", "tests", "assets")
PROBES = [
 "0^0", "TRUE&\"\"", "\"a\"<TRUE", "TRUE=1", "ISNUMBER(TRUE)", "TRUE+0", "FALSE<1", "\"\"=0",
 "DATE(30,1,1)", "DATE(50,1,1)", "DATE(1850,1,1)", "DATE(1900,1,1)", "DATE(1900,2,28)", "DATE(1900,2,29)", "DATE(1900,3,1)",
 "WEEKDAY(0)", "WEEKDAY(1)", "WEEKDAY(60)", "YEAR(0)", "MONTH(0)", "DAY(0)", "YEAR(1)", "DAY(1)", "DAY(60)", "TEXT(1,\"yyyy-mm-dd\")", "TEXT(61,\"yyyy-mm-dd\")",
 "\"10\"=10", "\"a\"=\"A\"", "\"abc\"<\"ABD\"", "MOD(5,0)", "ROUND(2.675,2)", "VALUE(\"\")", "LEN(123)", "LEFT(123,2)",
 "SEARCH(\"a?c\",\"abc\")", "SEARCH(\"a.c\",\"abc\")", "SEARCH(\"a*\",\"xabc\")", "COUNTIF(Data!B1:B8,\"a.*\")", "COUNTIF(Data!B1:B8,\"apple\")", "COUNTIF(Data!B1:B8,\"app\")",
 "INT(-0.5)", "2^0.5", "(-8)^(1/3)", "IF(\"\",1,2)", "IF(\"TRUE\",1,2)", "IF(\"a\",1,2)", "AND(\"a\")", "\"1\"+\"1\"", "TEXT(TRUE,\"0\")", "N(\"1\")",
 "SUM(TRUE)", "SUM(\"1\")", "SUM(Data!D1:D2)", "MAX(TRUE)", "COUNT(TRUE)", "COUNT(\"1\")", "COUNTA(\"\")", "ISBLANK(\"\")", "AVERAGE(\"a\")", "MIN(\"a\")",
 "\"[\"&123456789012345678&\"]\"", "\"[\"&(0.1*3)&\"]\"", "ROUND(-0.5,0)", "UPPER(\"ß\")", "PROPER(\"o'neil\")", "TRIM(CHAR(160)&\"a\")",
 "ISNUMBER(\"1\")", "ISTEXT(Data!D1)", "ISBLANK(Data!D1)", "Data!D1=0", "Data!D1=\"\"", "Data!D1&\"x\"", "Data!D1+1", "LEN(Data!D1)",
 "MATCH(\"A\",Data!B1:B8,0)", "VLOOKUP(\"APPLE\",Data!B1:C8,1,FALSE)", "FIND(\"A\",\"abc\")", "REPLACE(\"abc\",5,1,\"x\")", "MID(\"abc\",5,1)", "LEFT(\"abc\",-1)",
 "DATEDIF(DATE(2024,1,15),DATE(2024,3,10),\"MD\")", "DATEDIF(DATE(2020,2,29),DATE(2024,2,28),\"YD\")", "DATEDIF(DATE(2023,5,20),DATE(2024,3,10),\"YD\")", "DATEDIF(DATE(2024,3,10),DATE(2025,3,9),\"YD\")",
 "DATEVALUE(\"1 Mar 2024\")", "DATEVALUE(\"March 1, 2024\")", "DATEVALUE(\"1-Mar-2024\")", "DATEVALUE(\"2024/03/01\")", "DATEVALUE(\"3/1/24\")", "DATEVALUE(\"3/1/99\")", "DATEVALUE(\"2/30/2024\")",
 "VALUE(\"13:30\")", "VALUE(\"1:30 PM\")", "VALUE(\"3/1/2024 13:30\")", "\"3/1/2024\"+1", "YEAR(\"3/1/2024\")", "DATE(29,1,1)", "DATE(30,1,1)",
 "LOWER(\"ẞ\")", "UPPER(\"ñ\")", "PROPER(\"ßa\")", "UPPER(\"ǆ\")",
 "COUNTIF(Data!B1:B8,\"a*\")", "COUNTIF(Data!B1:B8,\"APPLE\")", "MATCH(\"APPLE\",Data!B1:B8,0)", "\"a\"=\"A\"", "VLOOKUP(\"ap.*\",Data!B1:C8,1,FALSE)", "SEARCH(\"B.N\",\"abnormal\")", "SUMIF(Data!B1:B8,\"an\",Data!A1:A8)",
 "EDATE(DATE(2024,1,31),1)", "EOMONTH(DATE(2024,1,31),-1)", "DATEDIF(DATE(2024,1,31),DATE(2024,3,1),\"MD\")", "_xlfn.DAYS(\"2024-03-01\",\"2024-02-01\")", "DATEVALUE(\"2024-03-01\")", "DATEVALUE(\"3/1/2024\")",
 "TEXT(1234.5,\"0.0\")", "TEXT(0.5,\"h:mm AM/PM\")", "TEXT(-1,\"0;(0)\")", "TEXT(\"a\",\"0\")", "TEXT(1,\"\")", "FIXED(1234.5678)", "FIXED(-1234.5678,2,TRUE)", "ROUND(1234.5678,-2)",
 "PMT(0,10,100)", "FV(0,10,-100)", "NPV(0.1,100)", "IRR(Data!J1:J5)", "CEILING(-2.1,2)", "CEILING(2.1,-2)", "FLOOR(-2.1,2)", "FLOOR(2.1,-2)", "MOD(-3,-2)", "POWER(-2,0.5)", "SQRT(-0)", "LOG(1,1)", "LN(1)", "FACT(171)",
 "ROW(Data!A5:A6)", "COLUMNS(Data!A1:C1)", "ADDRESS(1,1,1,FALSE)", "SUMPRODUCT(Data!A1:A3,Data!B1:B3)", "SUMPRODUCT(Data!A1:A3*1,Data!A1:A3)", "SUM(IF(Data!A1:A3>3,1,0))",
 "COUNTIF(Data!A1:A8,\"=5\")", "COUNTIF(Data!A1:A8,\">=5\")", "COUNTIF(Data!A1:A8,5)", "COUNTIF(Data!B1:B8,\"<>apple\")", "COUNTIF(Data!B1:B8,\"\")", "SUMIF(Data!A1:A8,\">4\")", "COUNTIF(Data!B1:B8,\"*an*\")", "COUNTIF(Data!B1:B8,\"a~*\")",
]

def strip_cached(content):
    """Drop cached results of formula cells so LibreOffice recalculates them."""
    def clean(m):
        t = m.group(0)
        if "table:formula=" not in t:
            return t
        return re.sub(r' (office:value-type|office:value|office:string-value|office:boolean-value|office:date-value|office:time-value|calcext:value-type)="[^"]*"', "", t)
    content = re.sub(r"<table:table-cell [^>]*>", clean, content)
    return re.sub(r"(<table:table-cell [^>]*table:formula=[^>]*>)<text:p>.*?</text:p>", r"\1", content)


def rewrite_ods(src, dst, settings):
    """Copy an ODS replacing its calculation settings and dropping cached values."""
    zin = zipfile.ZipFile(src)
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as zout:
        for item in zin.infolist():
            data = zin.read(item.filename)
            if item.filename == "content.xml":
                text = re.sub(r"<table:calculation-settings[^>]*>", settings, data.decode(), count=1)
                data = strip_cached(text).encode()
            zout.writestr(item, data, compress_type=zipfile.ZIP_STORED if item.filename == "mimetype" else zipfile.ZIP_DEFLATED)


SCENARIOS = {
    "probe_regex": '<table:calculation-settings table:case-sensitive="true" table:automatic-find-labels="false" table:use-regular-expressions="true" table:use-wildcards="false" table:search-criteria-must-apply-to-whole-cell="false">',
    "probe_literal": '<table:calculation-settings table:case-sensitive="false" table:automatic-find-labels="false" table:use-regular-expressions="false" table:use-wildcards="false">',
}


def circular(tmp, iterate):
    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Loop"
    ws["A1"], ws["B1"], ws["C1"] = "=B1/2+10", "=A1", "=A1+B1"
    wb.calculation = CalcProperties(iterate=iterate, iterateCount=50, iterateDelta=0.0001, fullCalcOnLoad=True)
    name = "circular_on" if iterate else "circular_off"
    src = os.path.join(tmp, name + ".xlsx")
    wb.save(src)
    for fmt in ("xlsx", "ods"):
        subprocess.run(["soffice", "--headless", "--convert-to", fmt, "--outdir", os.path.join(tmp, "o"), src], check=True, capture_output=True)
        os.replace(os.path.join(tmp, "o", f"{name}.{fmt}"), os.path.join(OUT, f"{name}.{fmt}"))


def main():
    wb = openpyxl.Workbook(); d = wb.active; d.title = "Data"
    for i, (n, t) in enumerate(zip([5,3,8,1,9,2,7,4], ["apple","banana","cherry","apple","date","banana","apple","fig"]), 1):
        d[f"A{i}"] = n; d[f"B{i}"] = t; d[f"C{i}"] = 45000 + i
    for i, v in enumerate([-100, 30, 40, 50, 60], 1): d[f"J{i}"] = v
    c = wb.create_sheet("Calc")
    for i, f in enumerate(PROBES, 1):
        c[f"A{i}"] = "=" + f; c[f"B{i}"] = f; c[f"B{i}"].data_type = "s"
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "probe.xlsx"); wb.save(src)
        for fmt in ("xlsx", "ods"):
            subprocess.run(["soffice","--headless","--convert-to",fmt,"--outdir",os.path.join(tmp,"o"),src],check=True,capture_output=True)
            if fmt == "ods":
                os.replace(os.path.join(tmp,"o","probe.ods"), os.path.join(OUT, "probe.ods"))
        for name, settings in SCENARIOS.items():
            raw = os.path.join(tmp, name + "_raw.ods")
            rewrite_ods(os.path.join(OUT, "probe.ods"), raw, settings)
            subprocess.run(["soffice","--headless","--convert-to","ods","--outdir",os.path.join(tmp,"o2"),raw],check=True,capture_output=True)
            os.replace(os.path.join(tmp,"o2",name + "_raw.ods"), os.path.join(OUT, name + ".ods"))
        circular(tmp, True)
        circular(tmp, False)
    print(len(PROBES), "probes")
main()
