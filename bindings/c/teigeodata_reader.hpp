// Reads a teigeodata result blob in place: the C++17 face of teigeodata_reader.h, header-only.
// Hand-written: the blob format is not part of the IDL. The same shape as every language's
// reader (D-322).
//
//     teigeodata::Blob blob = engine.search("Pokhara", 5);
//     for (auto place : teigeodata::Reader(blob)) {
//         std::cout << *place.string("name") << ' ' << *place.number("lat") << '\n';
//     }
//
// The reader, its rows and every std::string_view it gives borrow the blob: keep the blob
// alive while they are used (a reader cannot be made from a temporary Blob). A Row copies the
// reader's few words, so it stays valid after the Reader it came from is gone:
// `auto first = tg::Reader(blob)[0];` is fine while `blob` lives.
#ifndef TEIGEODATA_READER_HPP
#define TEIGEODATA_READER_HPP

#include <cstddef>
#include <cstdint>
#include <iterator>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "teigeodata.hpp"
#include "teigeodata_reader.h"

namespace teigeodata {

/// A value of a result: null, an integer, a floating-point number or a string in the blob.
using Value = std::variant<std::monostate, std::int64_t, double, std::string_view>;

/// One row as a map from column name to value.
using Record = std::map<std::string, Value, std::less<>>;

/// Reads a result blob: its rows and named columns. The constructor checks the layout, so no
/// read leaves the blob.
class Reader {
  public:
    /// Checks `blob`'s layout; throws Error(ErrorCode::Corrupt) for a malformed blob.
    explicit Reader(const Blob &blob) : Reader(blob.data(), blob.size()) {}
    Reader(const Blob &&) = delete;  // the reader would outlive the blob

    /// Checks the bytes' layout; throws Error(ErrorCode::Corrupt) for a malformed blob.
    Reader(const std::uint8_t *data, std::size_t size) {
        if (tg_reader_init(&r_, data, size) != 0) throw Error(ErrorCode::Corrupt, "not a well-formed result blob");
    }

    /// The number of rows.
    std::uint32_t rows() const noexcept { return r_.rows; }

    /// The column names, in order.
    std::vector<std::string> columns() const {
        std::vector<std::string> out;
        for (std::uint32_t i = 0; i < r_.columns; i++) {
            tg_str s = tg_reader_name(&r_, static_cast<std::int32_t>(i));
            out.emplace_back(s.ptr, s.len);
        }
        return out;
    }

    /// Whether there is a column called `name`.
    bool has(std::string_view name) const noexcept { return find(name) >= 0; }

    /// Per row, whether the column called `name` is null there.
    std::vector<bool> nulls(std::string_view name) const {
        const std::int32_t c = column_index(name);
        std::vector<bool> out(r_.rows);
        for (std::uint32_t i = 0; i < r_.rows; i++) out[i] = tg_reader_is_null(&r_, c, i);
        return out;
    }

    /// A number column as doubles (exact to 2^53 for a 64-bit integer); a null row reads as 0.
    std::vector<double> numbers(std::string_view name) const {
        const std::int32_t c = number_column(name);
        std::vector<double> out(r_.rows);
        for (std::uint32_t i = 0; i < r_.rows; i++) out[i] = tg_reader_number(&r_, c, i);
        return out;
    }

    /// A string column, std::nullopt where null.
    std::vector<std::optional<std::string_view>> strings(std::string_view name) const {
        const std::int32_t c = column_index(name);
        if (!tg_reader_is_text(&r_, c)) throw Error(ErrorCode::InvalidArgument, "column " + std::string(name) + " does not hold strings");
        std::vector<std::optional<std::string_view>> out(r_.rows);
        for (std::uint32_t i = 0; i < r_.rows; i++) out[i] = text(c, i);
        return out;
    }

    /// The column called `name` with one value per row.
    std::vector<Value> column(std::string_view name) const {
        const std::int32_t c = column_index(name);
        std::vector<Value> out(r_.rows);
        for (std::uint32_t i = 0; i < r_.rows; i++) out[i] = value(c, i);
        return out;
    }

    /// Every row as a Record.
    std::vector<Record> records() const {
        std::vector<Record> out(r_.rows);
        for (std::uint32_t c = 0; c < r_.columns; c++) {
            tg_str s = tg_reader_name(&r_, static_cast<std::int32_t>(c));
            const std::string name(s.ptr, s.len);
            for (std::uint32_t i = 0; i < r_.rows; i++) out[i].emplace(name, value(static_cast<std::int32_t>(c), i));
        }
        return out;
    }

    /// One row, read by column name without a copy.
    class Row {
      public:
        /// The value of column `name`; std::monostate where null or where there is none.
        Value operator[](std::string_view name) const { return value_of(r_, find_in(r_, name), row_); }
        /// A string column's value; std::nullopt where null or for a number column.
        std::optional<std::string_view> string(std::string_view name) const { return text_of(r_, find_in(r_, name), row_); }
        /// A number column's value; std::nullopt where null or for a string column.
        std::optional<double> number(std::string_view name) const {
            const std::int32_t c = find_in(r_, name);
            if (c < 0 || tg_reader_is_text(&r_, c) || tg_reader_is_null(&r_, c, row_)) return std::nullopt;
            return tg_reader_number(&r_, c, row_);
        }
        /// An integer column's value; std::nullopt as for number.
        std::optional<std::int64_t> integer(std::string_view name) const {
            const std::int32_t c = find_in(r_, name);
            if (c < 0 || tg_reader_is_text(&r_, c) || tg_reader_is_null(&r_, c, row_)) return std::nullopt;
            return tg_reader_int(&r_, c, row_);
        }
        /// Whether column `name` is null here (or there is none).
        bool is_null(std::string_view name) const { return tg_reader_is_null(&r_, find_in(r_, name), row_); }
        /// The row's index.
        std::uint32_t index() const noexcept { return row_; }

      private:
        friend class Reader;
        Row(const tg_reader &r, std::uint32_t row) noexcept : r_(r), row_(row) {}
        tg_reader r_;
        std::uint32_t row_;
    };

    /// Iterates the rows.
    class iterator {
      public:
        using iterator_category = std::forward_iterator_tag;
        using value_type = Row;
        using difference_type = std::ptrdiff_t;
        using pointer = void;
        using reference = Row;
        Row operator*() const noexcept { return Row(r_, row_); }
        iterator &operator++() noexcept {
            ++row_;
            return *this;
        }
        iterator operator++(int) noexcept {
            iterator old = *this;
            ++row_;
            return old;
        }
        bool operator==(const iterator &o) const noexcept { return row_ == o.row_ && r_.data == o.r_.data; }
        bool operator!=(const iterator &o) const noexcept { return !(*this == o); }

      private:
        friend class Reader;
        iterator(const tg_reader &r, std::uint32_t row) noexcept : r_(r), row_(row) {}
        tg_reader r_;
        std::uint32_t row_;
    };

    iterator begin() const noexcept { return iterator(r_, 0); }
    iterator end() const noexcept { return iterator(r_, r_.rows); }
    /// Row `i`.
    Row operator[](std::uint32_t i) const noexcept { return Row(r_, i); }

  private:
    static std::int32_t find_in(const tg_reader &r, std::string_view name) noexcept {
        if (name.empty() || name.size() > 16) return -1;
        for (std::uint32_t i = 0; i < r.columns; i++) {
            tg_str s = tg_reader_name(&r, static_cast<std::int32_t>(i));
            if (std::string_view(s.ptr, s.len) == name) return static_cast<std::int32_t>(i);
        }
        return -1;
    }

    std::int32_t find(std::string_view name) const noexcept { return find_in(r_, name); }

    std::int32_t column_index(std::string_view name) const {
        const std::int32_t c = find(name);
        if (c < 0) throw Error(ErrorCode::InvalidArgument, "no column " + std::string(name));
        return c;
    }

    std::int32_t number_column(std::string_view name) const {
        const std::int32_t c = column_index(name);
        if (tg_reader_is_text(&r_, c)) throw Error(ErrorCode::InvalidArgument, "column " + std::string(name) + " does not hold numbers");
        return c;
    }

    static std::optional<std::string_view> text_of(const tg_reader &r, std::int32_t c, std::uint32_t row) noexcept {
        tg_str s = tg_reader_string(&r, c, row);
        if (s.ptr == nullptr) return std::nullopt;
        return std::string_view(s.ptr, s.len);
    }

    std::optional<std::string_view> text(std::int32_t c, std::uint32_t row) const noexcept { return text_of(r_, c, row); }

    static Value value_of(const tg_reader &r, std::int32_t c, std::uint32_t row) noexcept {
        if (c < 0 || tg_reader_is_null(&r, c, row)) return std::monostate{};
        switch (tg_reader_type(&r, c)) {
        case TG_COLUMN_UTF8: case TG_COLUMN_DICT16: case TG_COLUMN_DICT32: {
            const auto t = text_of(r, c, row);
            if (t) return *t;
            return std::monostate{};
        }
        case TG_COLUMN_F32: case TG_COLUMN_F64: return tg_reader_number(&r, c, row);
        default: return tg_reader_int(&r, c, row);
        }
    }

    Value value(std::int32_t c, std::uint32_t row) const noexcept { return value_of(r_, c, row); }

    tg_reader r_{};
};

}  // namespace teigeodata

#endif  // TEIGEODATA_READER_HPP
