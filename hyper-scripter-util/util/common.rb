require 'io/console'

RED = "\033[0;31m".freeze
YELLOW_BG = "\033[0;43m".freeze
YELLOW_BG_RED = "\033[31;43m".freeze
NC = "\033[0m".freeze
ENTER = "\r".freeze

class HSEnv
  ENV_MAP = { name: 'NAME', cmd: 'HS_CMD', run_id: 'HS_RUN_ID',
              source: 'HS_SOURCE', home: 'HS_HOME', exe: 'HS_EXE' }.freeze

  def initialize(script_dir = nil)
    find_hs_env(script_dir)
    warn "hyper script home = #{@home}, executable = #{@exe}"
    @prefix = ''
  end

  def prefix(pref)
    @prefix = pref
  end

  attr_reader :home, :exe

  def do_hs(arg, all)
    cmd = hs_command_str(arg, all)
    output = `#{cmd}`
    raise StandardError, "Hyper scripter exits with #{$?.exitstatus}" unless $?.success?

    output
  end

  def env_var(var_name)
    k = ENV_MAP[var_name]
    v = ENV[k]
    raise StandardError, "No environment variable #{k} found" if v.nil?

    v
  end

  def exec_hs(arg, all = true)
    cmd = hs_command_str(arg, all)
    exec cmd.to_s
  end

  private

  def find_hs_env(script_dir)
    @home = if script_dir.nil?
              env_var(:home)
            else
              script_dir
            end
    @exe = env_var(:exe)
  end

  def hs_command_str(arg, all)
    visible_str = if all
                    '-f all --timeless'
                  else
                    ''
                  end
    "#{@exe} --no-alias -H #{@home} #{visible_str} #{@prefix} #{arg}"
  end
end

HS_ENV = HSEnv.new

# selector
class Selector
  class Empty < StandardError
  end

  class Quit < StandardError
  end

  def load(options)
    @options = options
  end

  # Handle customized keys
  def register_keys(keys, callback, msg: '', recur: false)
    @enter_overriden = true if keys.include?(ENTER)
    keys = [keys] unless keys.is_a?(Array)
    keys.each { |k| @callbacks.store(k, self.class.make_callback(callback, msg, recur)) }
  end

  # Initiate the selector
  # @param offset [Integer, #read] the first visual number of the candidates
  def initialize(options, offset = 1)
    load(options)
    @search_string = ''
    @number = nil
    @offset = offset
    @callbacks = {}
    @enter_overriden = false
    @multi_select_callback = nil
  end

  def on_multi_select(callback)
    @multi_select_callback= callback
  end

  def can_virtual?
    not @multi_select_callback.nil?
  end

  def run(sequence: '')
    pos = 0
    mode = :normal
    virtual_state = nil
    loop do
      win_width = IO.console.winsize[1]
      option_count = @options.length
      raise Empty if option_count == 0

      line_count = 0
      display_pos = @offset + pos

      virtual_state.set_point(display_pos) unless virtual_state.nil?

      if sequence.length == 0
        @options.each_with_index do |option, i|
          cur_display_pos = @offset + i
          is_virtual_selected = virtual_state.nil? ? false : virtual_state.in_range(cur_display_pos)
          leading = pos == i ? '>' : ' '
          gen_line = ->(content) { "#{leading} #{cur_display_pos}. #{content}" }
          line_count += compute_lines(gen_line.call(option).length, win_width) # calculate line height without color, since colr will mess up char count
          option = self.class.color_line(option, @search_string, is_virtual_selected)
          option = gen_line.call(option)

          option = "#{YELLOW_BG}#{option}#{NC}" if is_virtual_selected
          $stderr.print("#{option}\n")
        end
      end

      case mode
      when :search
        $stderr.print "/#{@search_string}"
      when :number
        $stderr.print ":#{@number}"
      end

      resp = if sequence.length > 0
               ch = sequence[0]
               sequence = sequence[1..]
               ch
             else
               STDIN.getch
             end
      exit if resp == "\u0003" # Ctrl-C

      callback = nil

      if mode == :search
        case resp
        when "\b", "\c?"
          if @search_string.length == 0
            mode = :normal
          else
            @search_string = @search_string[0..-2]
          end
        when ENTER
          mode = :normal
        else
          @search_string += resp
          new_pos = search_index(pos)
          pos = new_pos unless new_pos.nil?
        end
      elsif mode == :number
        case resp
        when "\b", "\c?"
          if @number == 0
            mode = :normal
          else
            @number /= 10
          end
        when ENTER
          mode = :normal
          pos = [@number - @offset, 0].max
          pos = [pos, option_count - 1].min
        else
          @number = @number * 10 + resp.to_i if resp =~ /[0-9]/
        end
      else
        case resp
        when 'q', 'Q'
          raise Quit
        when 'j', 'J'
          pos = (pos + 1) % option_count
        when 'k', 'K'
          pos = (pos - 1 + option_count) % option_count
        when 'n'
          new_pos = search_index(pos + 1)
          pos = new_pos unless new_pos.nil?
        when 'N'
          new_pos = search_index(pos - 1, true)
          pos = new_pos unless new_pos.nil?
        when '/'
          mode = :search
          @search_string = ''
        when 'v', 'V'
          if virtual_state.nil?
            virtual_state = VirtualState.new(display_pos)
          else
            virtual_state = nil
          end
        else
          resp_to_i = resp.to_i
          if resp =~ /[0-9]/
            mode = :number
            @number = resp.to_i
          elsif (resp == ENTER) && !@enter_overriden
            return self.class.make_result(display_pos, @options[pos])
          else
            @callbacks.each do |key, cur_callback|
              next unless key == resp

              callback = cur_callback
              break
            end
          end
        end
      end

      if callback.nil? || callback.recur
        line_count.times do
          $stderr.print "\e[A"
        end
        $stderr.print "\r\e[J"
      end

      next unless callback

      callback.cb.call(display_pos, @options[pos])
      return self.class.make_result(display_pos, @options[pos]) unless callback.recur

      # for options count change
      new_options = @options.length
      pos = new_options - 1 if pos >= new_options
    end
  end

  def self.make_result(pos, content)
    ret = Struct.new(:pos, :content)
    ret.new(pos, content)
  end

  def self.make_callback(cb, content, recur)
    ret = Struct.new(:cb, :content, :recur)
    ret.new(cb, content, recur)
  end

  def self.color_line(option, search_string, is_virtual_selected)
    if is_virtual_selected
      if search_string.length > 0
        return option.gsub(search_string, "#{YELLOW_BG_RED}#{search_string}#{YELLOW_BG}")
      end
    elsif search_string.length > 0
      return option.gsub(search_string, "#{RED}#{search_string}#{NC}")
    end
    option
  end

  private

  def search_index(pos, reverse = false)
    len = @options.length
    (0..len).each do |i|
      i = if reverse
            (len + pos - i) % len
          else
            (i + pos) % len
          end
      return i if @options[i].include?(@search_string)
    end
    nil
  end

  def compute_lines(len, win_width)
    lines = 1 + len / win_width
    lines -= 1 if len % win_width == 0
    lines
  end
end

class VirtualState
  def initialize(num)
    @fixed = num
    @moving = num
  end

  def set_point(num)
    @moving = num
  end

  def get_range
    if @fixed < @moving
      [@fixed, @moving + 1]
    else
      [@moving, @fixed + 1]
    end
  end

  def in_range(num)
    from, to = get_range
    num >= from and num < to
  end
end
