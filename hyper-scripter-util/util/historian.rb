# [HS_HELP]: Interactively run script from history.
# [HS_HELP]:
# [HS_HELP]: e.g.:
# [HS_HELP]:     hs historian -f hs hs/test --limit 20

require 'json'
require_relative './common'

def split_args(args)
  index = args.index('--')
  if index.nil?
    ['', args.join(' ')]
  else
    [args[..index].join(' '), args[index + 1..].join(' ')]
  end
end

SEQUENCE, ARGS = split_args(ARGV)

# prevent the call to `util/historian` screw up historical query
# e.g. hs util/historian !
HS_ENV.do_hs("history rm-id #{HS_ENV.env_var(:run_id)}", false)

arg_obj_str = HS_ENV.do_hs("--dump-args history show #{ARGS}", false)
arg_obj = JSON.parse(arg_obj_str)
filters = arg_obj['filter']
timeless = arg_obj['timeless']
recent = arg_obj['recent']
show_obj = arg_obj['subcmd']['History']['subcmd']['Show']
script_query = show_obj['script']
offset = show_obj['offset']
limit = show_obj['limit']

# ask the actual script by ls command
filter_str = (filters.map { |s| "--filter #{s}" }).join(' ')
time_str = if recent.nil?
             timeless ? '--timeless' : ''
           else
             "--recent #{recent}"
           end
script_name = HS_ENV.do_hs(
  "#{time_str} #{filter_str} ls #{script_query} --grouping none --plain --name",
  false
).strip

warn "Historian for #{script_name}"

load_history = lambda do
  history = HS_ENV.do_hs("history show =#{script_name}! --limit #{limit} --offset #{offset}", false)
  history.lines.map { |s| s.strip }
end

sourcing = false
echoing = false
selector = Selector.new(load_history.call, offset + 1)

selector.register_keys(%w[d D], lambda { |pos, _|
  HS_ENV.do_hs("history rm =#{script_name}! #{pos}", false)
  selector.load(load_history.call)
}, msg: 'delete the history', recur: true)

selector.register_keys(%w[p P], lambda { |_, _|
  echoing = true
}, msg: 'print the argument to stdout')

selector.register_keys(%w[c C], lambda { |_, _|
  sourcing = true
}, msg: 'set next command')

selector.register_keys(%w[r R], lambda { |pos, _|
  sourcing = true
  HS_ENV.do_hs("history rm =#{script_name}! #{pos}", false)
}, msg: 'replce the argument')

selector.register_keys_virtual(%w[d D], lambda { |min, max, _|
  HS_ENV.do_hs("history rm =#{script_name}! #{min}..#{max}", false)
  selector.load(load_history.call)
}, msg: 'delete the history', recur: true)

args = begin
  res = selector.run(sequence: SEQUENCE)
  res.content
rescue Selector::Empty
  puts 'History is empty'
  exit
rescue Selector::Quit
  exit
end

cmd = "=#{script_name}! #{args}" # known issue: \n \t \" will not be handled properly
if sourcing
  File.open(HS_ENV.env_var(:source), 'w') do |file|
    case ENV['SHELL'].split('/').last
    when 'fish'
      cmd = "#{HS_ENV.env_var(:cmd)} #{cmd}"
      file.write("commandline #{cmd.inspect}")
    else
      warn "#{ENV['SHELL']} not supported"
    end
  end
elsif echoing
  puts args
else
  warn cmd
  history = HS_ENV.exec_hs(cmd, false)
end
