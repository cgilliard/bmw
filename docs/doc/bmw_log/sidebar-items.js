window.SIDEBAR_ITEMS = {"enum":[["LogConfigOption","This enum is used to get/set log settings after [`Log::init`] is called. The only setting that cannot be set after initialization is the [`LogConfigOption::FilePath`] setting. It is read only. Trying to write to it will result in an error. The function used to get these values is [`Log::get_config_option`] and the function used to set these values is [`Log::set_config_option`]."],["LogConfigOptionName","This enum contains the names of the configuration options. It is used in the [`Log::get_config_option`] function. See [`Log::get_config_option`] for further details."],["LogLevel","Standard 6 log levels."]],"macro":[["debug","Set [`crate::LogLevel`] to [`crate::LogLevel::Debug`] or log at the [`crate::LogLevel::Debug`] log level. If no parameters are specified the log level will be set. If a single parameter is specified, that string will be logged. If two or more parameters are specified, the first parameter is a format string, the additional parameters will be formatted based on the format string. Logging is done by the global logger which can be configured using the [`crate::log_init`] macro. If [`crate::log_init`] is not called, the default values are used."],["debug_all","Same as [`debug`] except that the [`crate::Log::log_all`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["debug_plain","Same as [`debug`] except that the [`crate::Log::log_plain`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["do_log","This macro is called by the [`fatal`], [`error`], [`warn`], [`info`], [`debug`] and [`trace`] macros and the ‘all’ / ‘plain’ macros to process logging. Generally those macros should be used in favor of calling this one directly."],["error","Set [`crate::LogLevel`] to [`crate::LogLevel::Error`] or log at the [`crate::LogLevel::Error`] log level. If no parameters are specified the log level will be set. If a single parameter is specified, that string will be logged. If two or more parameters are specified, the first parameter is a format string, the additional parameters will be formatted based on the format string. Logging is done by the global logger which can be configured using the [`crate::log_init`] macro. If [`crate::log_init`] is not called, the default values are used."],["error_all","Same as [`error`] except that the [`crate::Log::log_all`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["error_plain","Same as [`error`] except that the [`crate::Log::log_plain`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["fatal","Set [`crate::LogLevel`] to [`crate::LogLevel::Fatal`] or log at the [`crate::LogLevel::Fatal`] log level. If no parameters are specified the log level will be set. If a single parameter is specified, that string will be logged. If two or more parameters are specified, the first parameter is a format string, the additional parameters will be formatted based on the format string. Logging is done by the global logger which can be configured using the [`crate::log_init`] macro. If [`crate::log_init`] is not called, the default values are used."],["fatal_all","Same as [`fatal`] except that the [`crate::Log::log_all`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["fatal_plain","Same as [`fatal`] except that the [`crate::Log::log_plain`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["get_log_option","Get the current value of the specified log option. The single parameter must be of the type [`crate::LogConfigOptionName`]. The macro returns a [`crate::LogConfigOption`] on success and [`bmw_err::Error`] on error. See [`crate::Log::get_config_option`] which is the underlying function call for full details."],["info","Set [`crate::LogLevel`] to [`crate::LogLevel::Info`] or log at the [`crate::LogLevel::Info`] log level. If no parameters are specified the log level will be set. If a single parameter is specified, that string will be logged. If two or more parameters are specified, the first parameter is a format string, the additional parameters will be formatted based on the format string. Logging is done by the global logger which can be configured using the [`crate::log_init`] macro. If [`crate::log_init`] is not called, the default values are used."],["info_all","Same as [`info`] except that the [`crate::Log::log_all`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["info_plain","Same as [`info`] except that the [`crate::Log::log_plain`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["log","This macro is called by the [`fatal`], [`error`], [`warn`], [`info`], [`debug`] and [`trace`] macros to process logging. Generally those macros should be used in favor of calling this one directly."],["log_all","This macro is called by the [`fatal_all`], [`error_all`], [`warn_all`], [`info_all`], [`debug_all`] and [`trace_all`] macros to process logging. Generally those macros should be used in favor of calling this one directly."],["log_init","Initialize the global log. This macro takes a single parameter, if none are specified, the default [`crate::LogConfig`] is used. Note that if this macro is not called before logging occurs, the default configuration is used. After either this macro is called or the default is set via another logging macro, calling this macro again will result in an error. It usually makes sense to initialize this macro very early in the startup of an application so that no unanticipated logging occurs before this macro is called by mistake."],["log_plain","This macro is called by the [`fatal_plain`], [`error_plain`], [`warn_plain`], [`info_plain`], [`debug_plain`] and [`trace_plain`] macros to process logging. Generally those macros should be used in favor of calling this one directly."],["log_rotate","Rotate the global log. See [`crate::Log::rotate`] for full details on the underlying rotate function and log rotation in general."],["need_rotate","See if the global log needs to be rotated. See [`crate::Log::need_rotate`] for full details on the underlying need_rotate function."],["set_log_option","Configure the log with the specified [`crate::LogConfigOption`]. This macro takes a single argument. The macro returns () on success or [`bmw_err::Error`] on failure. See [`crate::Log::set_config_option`] which is the underlying function call for full details."],["trace","Set [`crate::LogLevel`] to [`crate::LogLevel::Trace`] or log at the [`crate::LogLevel::Trace`] log level. If no parameters are specified the log level will be set. If a single parameter is specified, that string will be logged. If two or more parameters are specified, the first parameter is a format string, the additional parameters will be formatted based on the format string. Logging is done by the global logger which can be configured using the [`crate::log_init`] macro. If [`crate::log_init`] is not called, the default values are used."],["trace_all","Same as [`trace`] except that the [`crate::Log::log_all`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["trace_plain","Same as [`trace`] except that the [`crate::Log::log_plain`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["u64","a u64 conversion macro"],["usize","a usize conversion macro"],["warn","Set [`crate::LogLevel`] to [`crate::LogLevel::Warn`] or log at the [`crate::LogLevel::Warn`] log level. If no parameters are specified the log level will be set. If a single parameter is specified, that string will be logged. If two or more parameters are specified, the first parameter is a format string, the additional parameters will be formatted based on the format string. Logging is done by the global logger which can be configured using the [`crate::log_init`] macro. If [`crate::log_init`] is not called, the default values are used."],["warn_all","Same as [`warn`] except that the [`crate::Log::log_all`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."],["warn_plain","Same as [`warn`] except that the [`crate::Log::log_plain`] function of the underlying logger is called instead of the the [`crate::Log::log`] function. See the [`crate::Log`] trait for details on each."]],"struct":[["LogBuilder","The publicly accessible builder struct. This is the only way a log can be created outside of this crate. See [`LogBuilder::build`]."],["LogConfig","The log configuration struct. Logs can only be built through the [`crate::LogBuilder::build`] function. This is the only parameter to that function. An example configuration with all parameters explicitly specified might look like this:"]],"trait":[["Log","The main trait implemented by the bmw logger. Some features include: color coding, timestamps, stdout/file, rotation by size and time, log levels, file/line number to help with debugging, millisecond precision, auto-rotation capabilities, backtraces, file headers and ability to delete log rotations. Most implementations can use the log macros in this library instead of using the logger directly."]]};