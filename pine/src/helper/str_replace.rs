pub fn str_replace(template: &'static str, args: Vec<String>) -> String {
    let mut str_list = vec![];
    let mut operate_str = template;
    loop {
        match template.find("{}") {
            None => break,
            Some(index) => {
                str_list.push(&operate_str[..index]);
                operate_str = &operate_str[index + 2..];
            }
        }
    }
    debug_assert!(str_list.len() == args.len() + 1);
    let mut last_list = vec![];
    let arg_len = args.len();
    for (i, arg) in args.into_iter().enumerate() {
        last_list.push(String::from(str_list[i]));
        last_list.push(arg);
    }
    last_list.push(String::from(str_list[arg_len]));
    last_list.join("")
}
