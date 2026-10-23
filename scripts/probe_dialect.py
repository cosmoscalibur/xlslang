#!/usr/bin/env python3
"""Write tests/assets/probe.{xlsx,ods}: formulas whose result may differ between
Excel and LibreOffice, recalculated by LibreOffice (see docs/FIDELITY.md)."""
import os, subprocess, tempfile
import openpyxl

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
 "EDATE(DATE(2024,1,31),1)", "EOMONTH(DATE(2024,1,31),-1)", "DATEDIF(DATE(2024,1,31),DATE(2024,3,1),\"MD\")", "DAYS(\"2024-03-01\",\"2024-02-01\")", "DATEVALUE(\"2024-03-01\")", "DATEVALUE(\"3/1/2024\")",
 "TEXT(1234.5,\"0.0\")", "TEXT(0.5,\"h:mm AM/PM\")", "TEXT(-1,\"0;(0)\")", "TEXT(\"a\",\"0\")", "TEXT(1,\"\")", "FIXED(1234.5678)", "FIXED(-1234.5678,2,TRUE)", "ROUND(1234.5678,-2)",
 "PMT(0,10,100)", "FV(0,10,-100)", "NPV(0.1,100)", "IRR(Data!J1:J5)", "CEILING(-2.1,2)", "CEILING(2.1,-2)", "FLOOR(-2.1,2)", "FLOOR(2.1,-2)", "MOD(-3,-2)", "POWER(-2,0.5)", "SQRT(-0)", "LOG(1,1)", "LN(1)", "FACT(171)",
 "ROW(Data!A5:A6)", "COLUMNS(Data!A1:C1)", "ADDRESS(1,1,1,FALSE)", "SUMPRODUCT(Data!A1:A3,Data!B1:B3)", "SUMPRODUCT(Data!A1:A3*1,Data!A1:A3)", "SUM(IF(Data!A1:A3>3,1,0))",
 "COUNTIF(Data!A1:A8,\"=5\")", "COUNTIF(Data!A1:A8,\">=5\")", "COUNTIF(Data!A1:A8,5)", "COUNTIF(Data!B1:B8,\"<>apple\")", "COUNTIF(Data!B1:B8,\"\")", "SUMIF(Data!A1:A8,\">4\")", "COUNTIF(Data!B1:B8,\"*an*\")", "COUNTIF(Data!B1:B8,\"a~*\")",
]

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
            os.replace(os.path.join(tmp,"o",f"probe.{fmt}"), os.path.join(OUT, f"probe.{fmt}"))
    print(len(PROBES), "probes")
main()
