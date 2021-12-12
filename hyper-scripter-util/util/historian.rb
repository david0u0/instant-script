# [HS_HELP]: Interactively run script from history.
# [HS_HELP]:
# [HS_HELP]: e.g.:
# [HS_HELP]:     hs historian -f hs hs/test --limit 20

require 'json'
require_relative './common'
require_relative './selector'

class Option
  def initialize(name, content, number, single)
    @content = content
    @number = number
    @name = name
    @single = single
  end

  def to_s
    if @single
      @content
    else
      "(#{@name}) #{@content}"
    end
  end

  attr_reader :number, :content, :name
end

class Historian < Selector
  attr_reader :script_name

  def history_show
    dir_str = @dir.nil? ? '' : "--dir #{@dir}"
    queries_str = @script_query.join(' ')
    HS_ENV.do_hs(
      "history show #{@root_args_str} --limit #{@limit} --offset #{@offset} \
      --with-name #{dir_str} #{queries_str}", false
    )
  end

  def initialize(args)
    arg_obj_str = HS_ENV.do_hs("--dump-args history show #{args}", false)
    arg_obj = JSON.parse(arg_obj_str)

    show_obj = arg_obj['subcmd']['History']['subcmd']['Show']
    @offset = show_obj['offset']
    @limit = show_obj['limit']
    @dir = show_obj['dir']
    @script_query = show_obj['queries']
    @single = @script_query.length == 1 && !@script_query[0].include?('*')

    root_args = arg_obj['root_args']
    filters = root_args['filter']
    timeless = root_args['timeless']
    recent = root_args['recent']
    # TODO: toggle

    filter_str = (filters.map { |s| "--filter #{s}" }).join(' ')
    time_str = if recent.nil?
                 timeless ? '--timeless' : ''
               else
                 "--recent #{recent}"
               end
    @root_args_str = "#{time_str} #{filter_str}"

    super(offset: @offset + 1)

    load_history
    register_all
  end

  def process_history(name, content, number)
    return nil if content == ''

    Option.new(name, content, number, @single)
  end

  def run(sequence: '')
    super(sequence: sequence)
  rescue Selector::Empty
    puts 'History is empty'
    exit
  rescue Selector::Quit
    exit
  end

  def run_as_main(sequence: '')
    sourcing = false
    echoing = false
    register_keys(%w[p P], lambda { |_, _|
      echoing = true
    }, msg: 'print the argument to stdout')

    register_keys(%w[c C], lambda { |_, _|
      sourcing = true
    }, msg: 'set next command')

    register_keys(%w[r R], lambda { |_, obj|
      raise 'delete for list query not supported' unless @single

      sourcing = true
      HS_ENV.do_hs("history rm =#{obj.name}! #{obj.number}", false)
    }, msg: 'replce the argument')

    option = run(sequence: sequence).content
    name = option.name
    args = option.content

    cmd = "=#{name}! -- #{args}" # known issue: \n \t \" will not be handled properly
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
  end

  def get_history
    history = history_show
    opts = history.lines.each_with_index.map do |s, i|
      s = s.strip
      name, _, content = s.partition(' ')
      process_history(name, content, i + @offset + 1)
    end
    opts.reject { |opt| opt.nil? }
  end

  def load_history
    load(get_history)
    @max_name_len = 0
    @options.each do |opt|
      @max_name_len = [@max_name_len, opt.name.length].max if defined?(opt.name)
    end
  end

  def register_all
    register_keys(%w[d D], lambda { |_, obj|
      raise 'delete for list query not supported' unless @single

      HS_ENV.do_hs("history rm =#{obj.name}! #{obj.number}", false)
      load_history
    }, msg: 'delete the history', recur: true)

    register_keys_virtual(%w[d D], lambda { |min, max, options|
      raise 'delete for list query not supported' unless @single

      last_num = nil
      options.each do |opt|
        # TODO: test this and try to make it work
        raise 'Not a continuous range!' unless last_num.nil? || (last_num + 1 == opt.number)

        last_num = opt.number
      end

      # FIXME: obj.number?
      HS_ENV.do_hs("history rm =#{options[0].name}! #{min + @offset + 1}..#{max + @offset + 1}", false)
      load_history
      exit_virtual
    }, msg: 'delete the history', recur: true)
  end

  # prevent the call to `util/historian` screw up historical query
  # e.g. hs util/historian !
  def self.humble_run_id
    HS_ENV.do_hs("history humble #{HS_ENV.env_var(:run_id)}", false)
  end

  def self.rm_run_id
    HS_ENV.do_hs("history rm-id #{HS_ENV.env_var(:run_id)}", false)
  end
end

if __FILE__ == $0
  Historian.humble_run_id
  def split_args(args)
    index = args.index('--')
    if index.nil?
      ['', args.join(' ')]
    else
      [args[0..index].join(' '), args[index + 1..-1].join(' ')]
    end
  end
  sequence, args = split_args(ARGV)

  historian = Historian.new(args)
  historian.run_as_main(sequence: sequence)
end
