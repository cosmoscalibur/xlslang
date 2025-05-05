Diseñar un código en Rust para la detección de errores en archivos de Excel.

Se debe tener presente que un archivo de Excel `.xlsx` es realmente un archivo
comprimido en formato ZIP que contiene varios archivos XML.

Se dispone de un archivo `xl/workbook.xml` que contiene información sobre la
estructura del libro de Excel. En el elemento `sheets` se relaciona cada hoja
con el elemento `sheet`, que posee los atributos de nombre (`name`) y el
identificador (`sheetId`). Debe tenerse presente que los nombres de las hojas se
encuentran sanitizados y al recuperarlos deben ser desanitizados. Ejemplo,
`&gt;` debe ser desanitizado a `>`. En esta mismo archivo está el elemento
`definedNames`, en el cual cada definición asociada al elemento `definedName` se
debe validar que su valor no esté con un error de referencia (termina `#ref!`) e
identificar la referencia afectada (atributo `name`).

Usando el `sheetId` se puede obtener el archivo
`xl/worksheets/sheet{sheetId}.xml` que contiene la información de la hoja. En
este archivo se encuentran las celdas con su contenido en el elemento `c`. Cada
celda tiene un atributo `r` que indica su posición en la hoja. Ejemplo, `A1` es
la celda en la primera fila y primera columna. Si esta celda posee algún tipo de
error, el atributo de tipo `t` será `"e"`. En caso de tener una fórmula, tendrán
un elemento `f` que contiene la fórmula.

Requisitos:

Debe existir una función `open_zip_file` para abrir el objeto comprimido que
recibe la ruta del archivo. Esta ruta puede ser relativa o absoluta.

Debe existir una función `xlsx_mapping` que recibe el archivo ZIP y devuelve una
estructura que relaciona las hojas (nombre de hoja con identificador) y los
nombres definidos (nombre definido con valor) si el atributo `hidden` es
`"false"`.

Debe existir una función `error_in_sheet` que recibe el contenido del archivo de
una hoja y devuelve un vector de las celdas con errores de dicha hoja.

Debe existir una función `error_in_defined_name` que recibe la relación de
nombres definidos y devuelve un vector de los nombres definidos con errores.

Debe existir una función `error_in_all_sheets` que recibe el archivo ZIP y
devuelve un vector de las celdas con errores de todas las hojas.

Debe existir una función `workbook_errors` que recibe el archivo ZIP y devuelve
todos los errores entre las hojas y los nombres definidos.
