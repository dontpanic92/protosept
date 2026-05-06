mod array;
mod display;
mod hashmap;
mod string;
mod tuple;

use crate::interpreter::context::Context;

pub(crate) fn register_builtin_functions(ctx: &mut Context) {
    ctx.register_host_function("string.len_bytes".to_string(), string::string_len_bytes);
    ctx.register_host_function("string.display".to_string(), string::string_display);
    ctx.register_host_function("string.concat".to_string(), string::string_concat);
    ctx.register_host_function("string.len_chars".to_string(), string::string_len_chars);
    ctx.register_host_function("string.substring".to_string(), string::string_substring);
    ctx.register_host_function("string.char_at".to_string(), string::string_char_at);
    ctx.register_host_function("string.split".to_string(), string::string_split);
    ctx.register_host_function("string.index_of".to_string(), string::string_index_of);
    ctx.register_host_function("string.starts_with".to_string(), string::string_starts_with);
    ctx.register_host_function("string.contains".to_string(), string::string_contains);
    ctx.register_host_function("string.ends_with".to_string(), string::string_ends_with);
    ctx.register_host_function("string.repeat".to_string(), string::string_repeat);
    ctx.register_host_function("string.trim".to_string(), string::string_trim);
    ctx.register_host_function("string.trim_start".to_string(), string::string_trim_start);
    ctx.register_host_function("string.trim_end".to_string(), string::string_trim_end);
    ctx.register_host_function("display.int".to_string(), display::display_int);
    ctx.register_host_function("display.float".to_string(), display::display_float);
    ctx.register_host_function("display.bool".to_string(), display::display_bool);
    ctx.register_host_function("display.char".to_string(), display::display_char);
    ctx.register_host_function("display.unit".to_string(), display::display_unit);
    ctx.register_host_function("array.new".to_string(), array::array_new);
    ctx.register_host_function("array.index".to_string(), array::array_index);
    ctx.register_host_function("array.get".to_string(), array::array_get);
    ctx.register_host_function("array.len".to_string(), array::array_len);
    ctx.register_host_function("array.slice".to_string(), array::array_slice);
    ctx.register_host_function("array.push".to_string(), array::array_push);
    ctx.register_host_function("array.clear".to_string(), array::array_clear);
    ctx.register_host_function("array.pop".to_string(), array::array_pop);
    ctx.register_host_function("array.set".to_string(), array::array_set);
    ctx.register_host_function("array.insert".to_string(), array::array_insert);
    ctx.register_host_function("array.remove".to_string(), array::array_remove);
    ctx.register_host_function("array.index_of".to_string(), array::array_index_of);
    ctx.register_host_function("array.join".to_string(), array::array_join);
    ctx.register_host_function("array.map".to_string(), array::array_map);
    ctx.register_host_function("array.filter".to_string(), array::array_filter);
    ctx.register_host_function("array.reduce".to_string(), array::array_reduce);
    ctx.register_host_function("array.for_each".to_string(), array::array_for_each);
    ctx.register_host_function("array.find".to_string(), array::array_find);
    ctx.register_host_function("array.any".to_string(), array::array_any);
    ctx.register_host_function("array.all".to_string(), array::array_all);
    ctx.register_host_function(
        "builtin.entry_script_dir".to_string(),
        display::builtin_entry_script_dir,
    );
    ctx.register_host_function("builtin.min".to_string(), display::builtin_min);
    ctx.register_host_function("builtin.max".to_string(), display::builtin_max);
    ctx.register_host_function("builtin.clamp".to_string(), display::builtin_clamp);
    ctx.register_host_function("tuple.new".to_string(), tuple::tuple_new);
    ctx.register_host_function("tuple.index".to_string(), tuple::tuple_index);
    ctx.register_host_function("hashmap.new".to_string(), hashmap::hashmap_new);
    ctx.register_host_function("hashmap.len".to_string(), hashmap::hashmap_len);
    ctx.register_host_function("hashmap.get".to_string(), hashmap::hashmap_get);
    ctx.register_host_function("hashmap.set".to_string(), hashmap::hashmap_set);
    ctx.register_host_function("hashmap.remove".to_string(), hashmap::hashmap_remove);
    ctx.register_host_function(
        "hashmap.contains_key".to_string(),
        hashmap::hashmap_contains_key,
    );
    ctx.register_host_function("hashmap.keys".to_string(), hashmap::hashmap_keys);
    ctx.register_host_function("hashmap.values".to_string(), hashmap::hashmap_values);
    ctx.register_host_function("hashmap.index".to_string(), hashmap::hashmap_index);
}
