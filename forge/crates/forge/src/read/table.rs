//! A Parquet table read as named text columns, streamed a batch at a time: the official lists
//! (`towns.yaml`, D-259) name their columns as data, so one reader serves every country's lists.

use std::fs::File;
use std::path::Path;

use arrow_array::cast::AsArray;
use arrow_array::{Array, RecordBatch};
use arrow_schema::DataType;
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

/// Calls `f` with each row's values of `cols`, in that order; a null reads as `""`. Text
/// columns of any Arrow string type are read, others refused by name.
pub(crate) fn for_each_row(
    path: &Path,
    cols: &[&str],
    mut f: impl FnMut(&[&str]) -> Result<(), String>,
) -> Result<(), String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = File::open(path).map_err(|e| at(&e))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| at(&e))?;
    let fields = builder.schema().fields();
    let idx: Vec<usize> = cols
        .iter()
        .map(|n| {
            fields
                .iter()
                .position(|c| c.name() == n)
                .ok_or_else(|| at(&format!("no column `{n}`")))
        })
        .collect::<Result<_, _>>()?;
    let mask = ProjectionMask::roots(builder.parquet_schema(), idx);
    for batch in builder.with_projection(mask).build().map_err(|e| at(&e))? {
        let batch = batch.map_err(|e| at(&e))?;
        let arrays = cols
            .iter()
            .map(|n| text(&batch, n).map_err(|e| at(&e)))
            .collect::<Result<Vec<_>, _>>()?;
        // One buffer a batch: the row's values borrow the batch.
        let mut row: Vec<&str> = Vec::with_capacity(cols.len());
        for i in 0..batch.num_rows() {
            row.clear();
            row.extend(arrays.iter().map(|a| a.value(i)));
            f(&row)?;
        }
    }
    Ok(())
}

/// One text column of a batch, whatever its Arrow string type.
enum Text<'a> {
    Utf8(&'a arrow_array::StringArray),
    Large(&'a arrow_array::LargeStringArray),
    View(&'a arrow_array::StringViewArray),
}

impl Text<'_> {
    fn value(&self, i: usize) -> &str {
        match self {
            Self::Utf8(a) if a.is_valid(i) => a.value(i),
            Self::Large(a) if a.is_valid(i) => a.value(i),
            Self::View(a) if a.is_valid(i) => a.value(i),
            _ => "",
        }
    }
}

fn text<'a>(b: &'a RecordBatch, name: &str) -> Result<Text<'a>, String> {
    let c = b
        .column_by_name(name)
        .ok_or_else(|| format!("no column `{name}`"))?;
    match c.data_type() {
        DataType::Utf8 => Ok(Text::Utf8(c.as_string::<i32>())),
        DataType::LargeUtf8 => Ok(Text::Large(c.as_string::<i64>())),
        DataType::Utf8View => Ok(Text::View(c.as_string_view())),
        t => Err(format!("column `{name}` is {t}, not text")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::for_each_row;
    use arrow_array::{Int32Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;
    use std::sync::Arc;

    fn file(dir: &std::path::Path) -> std::path::PathBuf {
        let schema = Arc::new(Schema::new(vec![
            Field::new("code", DataType::Utf8, true),
            Field::new("name", DataType::Utf8, true),
            Field::new("n", DataType::Int32, true),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![Some("9"), Some("36")])),
                Arc::new(StringArray::from(vec![Some("Uttar Pradesh"), None])),
                Arc::new(Int32Array::from(vec![1, 2])),
            ],
        )
        .unwrap();
        let path = dir.join("t.parquet");
        let mut w =
            ArrowWriter::try_new(std::fs::File::create(&path).unwrap(), schema, None).unwrap();
        w.write(&batch).unwrap();
        w.close().unwrap();
        path
    }

    #[test]
    fn named_text_columns_read_in_order_nulls_empty() {
        let dir = std::env::temp_dir().join(format!("forge-table-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = file(&dir);
        let mut rows = Vec::new();
        for_each_row(&path, &["name", "code"], |r| {
            rows.push(r.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
            Ok(())
        })
        .unwrap();
        assert_eq!(rows, [["Uttar Pradesh", "9"], ["", "36"]]);
        let none = for_each_row(&path, &["nope"], |_| Ok(())).unwrap_err();
        assert!(none.contains("no column `nope`"), "{none}");
        let int = for_each_row(&path, &["n"], |_| Ok(())).unwrap_err();
        assert!(int.contains("not text"), "{int}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
