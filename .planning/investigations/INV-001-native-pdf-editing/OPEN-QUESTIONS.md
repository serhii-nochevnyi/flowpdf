# Open questions

- [ ] Which edit family should form the first end-to-end native-PDF tracer, and what exact bounded fixture demonstrates it? — owner: product/engineering
- [ ] When an edit requires rewriting a page with unsupported objects, should FlowPDF preserve a provably safe opaque object graph or refuse the edit for that page/file? — owner: product/engineering
- [ ] What exact conditions make a text island editable (font embedding/mapping, transforms, clipping, text-show operators, and replacement fit), and should overflow always be refused? — owner: engineering
- [ ] Does page insertion mean blank-page creation only, or importing/copying a page from another PDF as well? — owner: product
- [ ] Which content must secure redaction remove in the first supported subset: visible text/vector/image content only, or also hidden text, metadata, annotations, attachments, and other hidden structures? — owner: product/security
- [ ] Which independently maintained tools, fixtures, and visual thresholds are available for post-save redaction verification in local and release environments? — owner: engineering/release
- [ ] What is the supported annotation/widget set, and how should edits to signature widgets or signature-bearing documents be refused and explained? — owner: product/security
