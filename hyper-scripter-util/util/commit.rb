# frozen_string_literal: true

# [HS_HELP]: Create git commit for hyper-scripter home.
# [HS_HELP]:
# [HS_HELP]: The commit message is auto-generated by date and hyper-scripter home
# [HS_HELP]: e.g. [Auto Commit 2022-11-30 (my_scripts)]
# [HS_HELP]: You can modify this message so that the commit is not treated as auto-generated
# [HS_HELP]: Consecutive auto-generated commit with same message will be merged (using --amend)
# [HS_HELP]:
# [HS_HELP]: USAGE:
# [HS_HELP]:     hs commit

require_relative './common'

REAL_HS_HOME = File.realpath(HS_ENV.home)
Dir.chdir(REAL_HS_HOME)
GIT_HOME = run_cmd('git rev-parse --show-toplevel').chop
BRANCH = run_cmd('git rev-parse --abbrev-ref HEAD').chop
REMOTE = 'origin'
Dir.chdir(GIT_HOME)

def confirm(msg)
  loop do
    $stderr.print(msg, ' ')
    case read_char
    when 'y', 'Y'
      warn 'Y'
      return true
    when 'n', 'N'
      warn 'N'
      return false
    else
      warn 'Only Y and N is allowed'
    end
  end
end

def get_branch_state
  ahead = !run_cmd("git rev-list #{REMOTE}/#{BRANCH}..#{BRANCH}").chop.empty?
  behind = !run_cmd("git rev-list #{BRANCH}..#{REMOTE}/#{BRANCH}").chop.empty?
  if ahead && behind
    :diverged
  elsif ahead
    :ahead
  elsif behind
    :behind
  else
    :up_to_date
  end
end
system('git fetch --all', exception: true)
branch_state = get_branch_state
warn "branch state = #{branch_state}"

# Check if diverge
if branch_state == :diverged
  warn 'branch is diverged!'
  exit 1
end

def recur_check_dirty(file)
  return if File.identical?(REAL_HS_HOME, file)

  unless REAL_HS_HOME.start_with?(file)
    status = run_cmd("git status #{file} --porcelain")
    unless status.empty?
      warn status
      ok = confirm("#{file} is not clean. Sure to proceed? [Y/N]")
      exit 1 unless ok
    end
    return
  end

  Dir.foreach(file) do |f|
    next if ['.', '..', '.git'].include?(f)

    recur_check_dirty("#{file}/#{f}")
  end
end

recur_check_dirty(GIT_HOME)

if branch_state == :behind
  # Check if remote had changed
  system('git add -A', exception: true)
  system('git stash', exception: true)

  if File.exist?(REAL_HS_HOME) # else: hs home was just created, no need to check
    diff = run_cmd("git diff --stat #{REMOTE}/#{BRANCH} #{REAL_HS_HOME}").chop
    unless diff.empty?
      warn 'remote home had changed!'
      warn diff
      system('git stash pop', exception: true)
      exit 1
    end
  end

  # prepare the files
  system('git pull', exception: true)
  system('git stash pop', exception: true)
end

# create the commit
last_commit_msg = run_cmd("git log --pretty=format:'%s' --max-count 1")

date = Time.now.utc
date_str = date.strftime('%Y-%m-%d')
msg = "[Auto Commit #{date_str} (#{File.basename(REAL_HS_HOME)})]"

system('git add -A', exception: true)
if last_commit_msg.start_with?(msg)
  warn 'Amend the last commit'
else
  warn 'Create new commit'
  system("git commit -m '#{msg}'", exception: true)
end

system('git commit --amend', exception: true)