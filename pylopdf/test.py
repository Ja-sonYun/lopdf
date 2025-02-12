from pylopdf import pylopdf

pdf = pylopdf.Pdf("/Users/jasony/Downloads/有価証券報告書.pdf")
if pdf.is_encrypted():
    pdf.set_password("")
pdf.redact_text("株式会社")
pdf.save("./output.pdf")
